use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use asset_audio::CreateFxOneshot;
use asset_game::{FxDefinitions, FxName, OwnedFxEffectDef};
use bevy::tasks::ComputeTaskPool;
use fx::{
    FX_CATALOG_INDEX_NONE, FxBoltTarget, FxChildKind, FxCloudInstance, FxDrawElemContext,
    FxDrawTrailContext, FxDrawTrailSampleContext, FxEffectDefInfo, FxElemDefInfo,
    FxElemLightInstance, FxElemMotionResult, FxElemTraceHit, FxGapCause, FxGenerateVertsOut,
    FxModelInstance, FxPackedLightingSrc, FxPlayPose, FxPlayRequest, FxSparkDrawQuery,
    FxSparkFillQuery, FxSparkFillVisual, FxSpriteInstance, FxSystemHost, FxTrailCollideHit,
    FxTrailDrawDef, FxTrailSampleVisual, PendingCollide, PlayResult,
    evaluate_elem_collide_motion, evaluate_elem_motion, generate_verts_with_trails, play_bolted,
    play_oriented, spawn_impact_or_death_effect, spawn_looping_partial, spawn_oriented,
    update as fx_update,
};
use fx_iw4::{
    FX_ELEM_ATLAS_OFF, FX_ELEM_ATLAS_SIZE, FX_RAND_CH_COLOR, FX_RAND_CH_SCALE, FX_RAND_CH_SIZE0,
    FxElemType, apply_lighting_frac_bgra, build_cloud, clamp_elem_rotation_time, cull_cloud,
    cull_elem_light, effect_def_needs_lighting_sample, elem_light_color_bgr,
    elem_uses_lighting_frac, evaluate_rotation_total, evaluate_scale, evaluate_size0,
    evaluate_size1, get_elem_angles_axis, get_velocity_at_time, random_table_f32, sprite_atlas_uv,
    vec3_normalize, vis_blocker_add_prepared,
};
use sim::SimWorld;

pub trait FxScene {
    fn sample_atpoint_rgb(&self, origin: [f32; 3]) -> Option<[u8; 3]>;
    fn finish_impact_marks(
        &self,
        host: &mut FxSystemHost,
        def_index: u8,
        against_world: bool,
        against_models: bool,
    );
    fn box_surfaces_prelude_ran(&self, host: &FxSystemHost) -> bool;
}

#[derive(Clone, Copy)]
pub struct FxDrawCull<'a> {
    pub elem_draw: bool,
    pub planes: &'a [[f32; 4]],
}

#[derive(Debug, Default, Clone)]
pub struct FxCreateFxBoot {
    pub named: u32,
    pub held: u32,
    pub miss_def: u32,

    pub failed: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct FxVertsGaps {
    pub lighting_frac: u32,

    pub lighting_frac_def_indices: Vec<u8>,

    pub lighting_applied: u32,

    pub lighting_white: u32,

    pub lighting_atpoint: u32,

    pub lighting_miss: u32,
    pub no_def: u32,
    pub no_elem: u32,

    pub no_material_visual: u32,
    pub no_size0_sample: u32,
    pub size0_not_positive: u32,
    pub no_size1_sample: u32,
    pub size1_not_positive: u32,

    pub color_fallback: u32,

    /// bo2mp: Modern Warfare Remastered sprites drawn by the game's rules,
    /// and those its rules leave out this frame (faded, no alpha or size).
    pub h1_drawn: u32,
    pub h1_not_drawn: u32,
    /// bo2mp: the same for Ghosts sprites under Ghosts' rules.
    pub iw6_drawn: u32,
    pub iw6_not_drawn: u32,
    /// Ghosts elements dropped by the exe's per-element draw cull.
    pub iw6_culled: u32,
    /// Ghosts sprites a material rule (falloff, near ramp, eye offset) touched.
    pub iw6_material: u32,

    pub catalog_index_n: u32,

    pub catalog_name_n: u32,
}

#[derive(Clone, Debug, Default)]
pub struct FxElemInfoCache {
    by_index: Vec<Arc<[FxElemDefInfo]>>,
    synced_len: usize,
    synced_first: Option<String>,
}

impl FxElemInfoCache {
    pub fn sync(&mut self, catalog: &FxDefinitions) {
        let first = catalog.names().next().map(str::to_owned);
        if self.synced_len == catalog.len() && self.synced_first == first {
            return;
        }
        self.by_index = catalog
            .effects()
            .map(|effect| Arc::<[FxElemDefInfo]>::from(owned_elem_infos(effect)))
            .collect();
        self.synced_len = catalog.len();
        self.synced_first = first;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn arc_for(
        &self,
        catalog: &FxDefinitions,
        effect: &OwnedFxEffectDef,
    ) -> Arc<[FxElemDefInfo]> {
        catalog
            .index_of(effect)
            .and_then(|index| self.by_index.get(index).cloned())
            .unwrap_or_else(|| Arc::from(owned_elem_infos(effect)))
    }
}

fn def_info<'a>(effect: &OwnedFxEffectDef, elems: &'a [FxElemDefInfo]) -> FxEffectDefInfo<'a> {
    FxEffectDefInfo {
        msec_looping_life: effect.view.msec_looping_life,
        looping_count: effect.view.looping_count,
        one_shot_count: effect.view.one_shot_count,
        emission_count: effect.view.emission_count,
        elems,
    }
}

fn stamp_packed_lighting(
    host: &mut FxSystemHost,
    handle: u16,
    catalog: &FxDefinitions,
    world: Option<&dyn FxScene>,
) {
    let Some(effect) = host.slot_for_handle(handle) else {
        return;
    };
    let index = effect.catalog_index;
    let origin = effect.origin;
    let needs = catalog_lookup(catalog, index)
        .is_some_and(|d| effect_def_needs_lighting_sample(d.view.flags));
    if !needs {
        return;
    }
    let rgb = world.and_then(|world| world.sample_atpoint_rgb(origin));
    let Some(slot) = host.slot_for_handle_mut(handle) else {
        return;
    };
    match rgb {
        Some(rgb) => {
            slot.packed_lighting = rgb;
            slot.packed_lighting_src = FxPackedLightingSrc::AtPointRgb;
        }
        None => {
            slot.packed_lighting_src = FxPackedLightingSrc::Missing;
        }
    }
}

fn stamp_play_lighting(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    world: Option<&dyn FxScene>,
    result: PlayResult,
) -> PlayResult {
    if let Some(handle) = result.handle() {
        stamp_packed_lighting(host, handle, catalog, world);
    }
    result
}

pub fn restamp_missing_packed_lighting(host: &mut FxSystemHost, world: Option<&dyn FxScene>) {
    let Some(world) = world else {
        return;
    };
    let pending: Vec<(u16, [f32; 3])> = host
        .live_effects()
        .filter(|e| e.packed_lighting_src == FxPackedLightingSrc::Missing)
        .map(|e| (e.own_handle, e.origin))
        .collect();
    for (handle, origin) in pending {
        let Some(rgb) = world.sample_atpoint_rgb(origin) else {
            continue;
        };
        if let Some(slot) = host.slot_for_handle_mut(handle) {
            slot.packed_lighting = rgb;
            slot.packed_lighting_src = FxPackedLightingSrc::AtPointRgb;
        }
    }
}

fn evaluate_color_rgba(samples: &[u8], intervals: u8, time: f32, random: f32) -> Option<[u8; 4]> {
    fx_iw4::evaluate_color_bgra(samples, intervals, time, random).map(|[b, g, r, a]| [r, g, b, a])
}

fn apply_elem_lighting(
    color: [u8; 4],
    lighting_frac: u8,
    packed: [u8; 3],
    src: FxPackedLightingSrc,
    def_index: u8,
    gaps: &mut FxVertsGaps,
) -> [u8; 4] {
    if !elem_uses_lighting_frac(lighting_frac) {
        return color;
    }
    gaps.lighting_frac = gaps.lighting_frac.saturating_add(1);
    if src == FxPackedLightingSrc::Missing {
        gaps.lighting_miss = gaps.lighting_miss.saturating_add(1);
        gaps.lighting_frac_def_indices.push(def_index);
        return color;
    }
    gaps.lighting_applied = gaps.lighting_applied.saturating_add(1);
    match src {
        FxPackedLightingSrc::White => {
            gaps.lighting_white = gaps.lighting_white.saturating_add(1);
        }
        FxPackedLightingSrc::AtPointRgb => {
            gaps.lighting_atpoint = gaps.lighting_atpoint.saturating_add(1);
        }
        FxPackedLightingSrc::Missing => {}
    }
    let [r, g, b, a] = color;
    let [b, g, r, a] = apply_lighting_frac_bgra([b, g, r, a], packed, lighting_frac);
    [r, g, b, a]
}

pub fn boot_createfx_effects(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    emitters: &[CreateFxOneshot],
    msec_now: i32,
    world: Option<&dyn FxScene>,
) -> FxCreateFxBoot {
    host.set_msec_now(msec_now);
    let mut census = FxCreateFxBoot::default();
    for shot in emitters {
        let Some(effect) = catalog.resolve_createfx_id(&shot.fxid) else {
            census.miss_def = census.miss_def.saturating_add(1);
            continue;
        };
        census.named = census.named.saturating_add(1);
        let elem_infos = cache.arc_for(catalog, effect);
        let result = spawn_oriented(
            host,
            FxPlayRequest {
                def_name: effect.name.as_str(),
                pose: FxPlayPose {
                    origin: shot.origin_inches,
                    axis: axis_from_angles_deg(shot.angles_deg),

                    msec: createfx_spawn_msec(msec_now, shot.delay),
                },
                wants_spotlight: false,
                catalog_index: catalog_index_of(catalog, effect),
                def: Some(def_info(effect, &elem_infos)),
            },
        );
        let result = stamp_play_lighting(host, catalog, world, result);
        if matches!(result, PlayResult::Held { .. }) {
            census.held = census.held.saturating_add(1);
        } else if let PlayResult::Failed(e) = result {
            census.failed.push(format!("{}: {e:?}", effect.name));
        }
    }
    drain_spawn_side_effects(host, catalog, cache, world);
    restamp_missing_packed_lighting(host, world);
    census
}

fn createfx_spawn_msec(msec_now: i32, delay_seconds: f32) -> i32 {
    msec_now.wrapping_add((delay_seconds * 1000.0) as i32)
}

pub fn play_named_oriented_in_world(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    name: FxName<'_>,
    origin: [f32; 3],
    axis: [[f32; 3]; 3],
    world: Option<&dyn FxScene>,
) -> Option<PlayResult> {
    let msec = host.msec_now;
    let result = play_named_at(host, catalog, cache, name, origin, axis, msec, world)?;
    drain_spawn_side_effects(host, catalog, cache, world);
    Some(result)
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_named_oriented_in_world(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    name: FxName<'_>,
    origin: [f32; 3],
    axis: [[f32; 3]; 3],
    msec: i32,
    world: Option<&dyn FxScene>,
) -> Option<PlayResult> {
    let effect = name.resolve(catalog)?;
    let elem_infos = cache.arc_for(catalog, effect);
    let result = spawn_oriented(
        host,
        FxPlayRequest {
            def_name: effect.name.as_str(),
            pose: FxPlayPose { origin, axis, msec },
            wants_spotlight: false,
            catalog_index: catalog_index_of(catalog, effect),
            def: Some(def_info(effect, &elem_infos)),
        },
    );
    let result = stamp_play_lighting(host, catalog, world, result);
    drain_spawn_side_effects(host, catalog, cache, world);
    Some(result)
}

#[allow(clippy::too_many_arguments)]
pub fn play_named_oriented_at_msec(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    name: FxName<'_>,
    origin: [f32; 3],
    axis: [[f32; 3]; 3],
    msec: i32,
    world: Option<&dyn FxScene>,
) -> Option<PlayResult> {
    let result = play_named_at(host, catalog, cache, name, origin, axis, msec, world)?;
    drain_spawn_side_effects(host, catalog, cache, world);
    Some(result)
}

pub fn play_named_bolted_in_world(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    name: FxName<'_>,
    target: FxBoltTarget,
    world: Option<&dyn FxScene>,
) -> Option<PlayResult> {
    let effect = name.resolve(catalog)?;
    let elem_infos = cache.arc_for(catalog, effect);
    let result = play_bolted(
        host,
        FxPlayRequest {
            def_name: effect.name.as_str(),
            pose: FxPlayPose {
                origin: target.orientation.origin,
                axis: target.orientation.axis,
                msec: host.msec_now,
            },
            wants_spotlight: false,
            catalog_index: catalog_index_of(catalog, effect),
            def: Some(def_info(effect, &elem_infos)),
        },
        target,
    );
    let result = stamp_play_lighting(host, catalog, world, result);
    drain_spawn_side_effects(host, catalog, cache, world);
    Some(result)
}

fn play_named_at(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    name: FxName<'_>,
    origin: [f32; 3],
    axis: [[f32; 3]; 3],
    msec: i32,
    world: Option<&dyn FxScene>,
) -> Option<PlayResult> {
    let effect = name.resolve(catalog)?;

    let elem_infos = cache.arc_for(catalog, effect);
    let result = play_oriented(
        host,
        FxPlayRequest {
            def_name: effect.name.as_str(),
            pose: FxPlayPose { origin, axis, msec },
            wants_spotlight: false,
            catalog_index: catalog_index_of(catalog, effect),
            def: Some(def_info(effect, &elem_infos)),
        },
    );
    Some(stamp_play_lighting(host, catalog, world, result))
}

pub fn drain_spawn_runners(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    world: Option<&dyn FxScene>,
) {
    const MAX_DRAIN: usize = 256;
    let mut steps = 0;
    while !host.pending_runners.is_empty() && steps < MAX_DRAIN {
        steps += 1;
        let batch = std::mem::take(&mut host.pending_runners);
        for req in batch {
            let Some(edge) = catalog_lookup(catalog, req.catalog_index)
                .and_then(|parent| parent.elems.get(req.def_index as usize))
                .and_then(|elem| elem.runner_child_edge(req.random_seed))
            else {
                skip_runner(host, &req);
                continue;
            };

            if edge.is_absent() {
                continue;
            }

            let Some(index) = edge.bound_index() else {
                skip_runner(host, &req);
                continue;
            };
            let Some(child) = catalog.def_at(index).filter(|d| !d.elems.is_empty()) else {
                skip_runner(host, &req);
                continue;
            };
            let child_name = child.name.clone();
            let previous_mark_entity = host.spawn_mark_entity;
            host.spawn_mark_entity = req.mark_entity;
            let played = play_def_at(
                host,
                catalog,
                cache,
                child,
                req.origin,
                req.axis,
                req.msec_begin,
                world,
            )
            .is_some();
            host.spawn_mark_entity = previous_mark_entity;
            if !played {
                skip_runner(host, &req);
                continue;
            }
            host.last_runner_child = Some(child_name);
            host.last_runner_msec = Some(req.msec_begin);
            host.last_runner_rot_deg = req.rot_deg;
        }
    }
}

fn play_def_at(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    effect: &OwnedFxEffectDef,
    origin: [f32; 3],
    axis: [[f32; 3]; 3],
    msec: i32,
    world: Option<&dyn FxScene>,
) -> Option<PlayResult> {
    if effect.elems.is_empty() {
        return None;
    }
    let elem_infos = cache.arc_for(catalog, effect);
    let result = play_oriented(
        host,
        FxPlayRequest {
            def_name: effect.name.as_str(),
            pose: FxPlayPose { origin, axis, msec },
            wants_spotlight: false,
            catalog_index: catalog_index_of(catalog, effect),
            def: Some(def_info(effect, &elem_infos)),
        },
    );
    Some(stamp_play_lighting(host, catalog, world, result))
}

fn skip_runner(host: &mut FxSystemHost, req: &fx::PendingRunnerSpawn) {
    host.last_runner_parent = Some(req.parent_name.clone()).filter(|name| !name.is_empty());
    host.last_runner_elem = Some(req.def_index);
    host.gaps.raise(FxGapCause::ElemRunnerSpawnSkipped {
        def_index: req.def_index,
    });
}

fn drain_spawn_side_effects(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    world: Option<&dyn FxScene>,
) {
    drain_spawn_runners(host, catalog, cache, world);
    drain_spawn_decals(host, catalog, world);
}

pub fn drain_spawn_decals(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    world: Option<&dyn FxScene>,
) {
    const MAX_DRAIN: usize = 256;
    let mut steps = 0;
    while !host.pending_decals.is_empty() && steps < MAX_DRAIN {
        steps += 1;
        let batch = std::mem::take(&mut host.pending_decals);
        for req in batch {
            record_spawn_decal(host, catalog, &req, world);
        }
    }
}

/// A distortion sprite as a quad for the native renderer: a billboard faces
/// the eye (half-extents `size0` wide, `size1` tall, or square when that is
/// 0), an oriented sprite spans its frame's axes 1 and 2; turned by the
/// rotation about its normal.
fn distortion_quad(view: &asset_world::Iw6FxView, center: [f32; 3], size: [f32; 2], elem_type: u8, axis: [[f32; 3]; 3], rotation: f32, color: [u8; 4], material: u16) -> asset_world::Iw6DistortionQuad {
    let (right, up) = if elem_type == 1 { (axis[1], axis[2]) } else { (view.right, view.up) };
    let (s, c) = rotation.sin_cos();
    let h = if size[1] > 0.0 { size[1] } else { size[0] };
    let mut r = [0.0f32; 3];
    let mut u = [0.0f32; 3];
    for k in 0..3 {
        r[k] = (right[k] * c + up[k] * s) * size[0];
        u[k] = (up[k] * c - right[k] * s) * h;
    }
    asset_world::Iw6DistortionQuad { center, right: r, up: u, color: u32::from_le_bytes([color[2], color[1], color[0], color[3]]), material }
}

/// The native Ghosts renderer's spawn for a decal element: the mark's
/// radius (the first size channel), axes (`fx_iw4::impact_mark_axis`, turned
/// by the rotation), colour and life, and the world material (the visual's
/// second material, as the world receiver takes it; the first when there is
/// only one). None when the element names no material.
fn iw6_decal_spawn(host: &FxSystemHost, catalog: &FxDefinitions, req: &fx::PendingDecalSpawn) -> Option<asset_world::Iw6DecalSpawn> {
    let elem = catalog_lookup(catalog, req.catalog_index).and_then(|parent| parent.elems.get(req.def_index as usize))?;
    let visual = elem.decal_mark_pair(req.random_seed)?;
    let name = visual.mark_hint(1).or_else(|| visual.mark_hint(0))?;
    let material = asset_world::iw6_decal_material_id(name)?;
    let radius = host.last_decal_size0.filter(|r| *r > 0.0)?;
    let axis = fx_iw4::impact_mark_axis(req.axis, host.last_decal_rotation.unwrap_or(0.0));
    Some(asset_world::Iw6DecalSpawn {
        origin: req.origin,
        axis,
        radius,
        color: host.last_decal_color.unwrap_or(0xffff_ffff),
        life_ms: u32::try_from(elem.view.life_span_msec_base).unwrap_or(0),
        material,
    })
}

fn record_spawn_decal(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    req: &fx::PendingDecalSpawn,
    world: Option<&dyn FxScene>,
) {
    host.last_mark_alloc_slot = None;
    host.last_decal_parent = Some(req.parent_name.clone()).filter(|name| !name.is_empty());
    host.last_decal_elem = Some(req.def_index);
    host.last_decal_msec = Some(req.msec_begin);
    host.last_decal_origin = Some(req.origin);
    host.last_decal_axis = Some(req.axis);
    host.last_decal_bolt = Some(req.bolt);
    host.last_decal_mark_entity = req.mark_entity;
    host.last_decal_vis = None;
    host.last_decal_size0 = None;
    host.last_decal_rotation = None;
    host.last_decal_mat0_edge = None;
    host.last_decal_mat1_edge = None;
    host.last_decal_mat0 = None;
    host.last_decal_mat1 = None;
    host.last_decal_color = None;

    if let Some(elem) = catalog_lookup(catalog, req.catalog_index)
        .and_then(|parent| parent.elems.get(req.def_index as usize))
    {
        host.last_decal_vis =
            Some(fx_iw4::elem_visual_index(elem.view.visual_count, req.random_seed) as u8);
        let rand_size = random_table_f32(req.random_seed, FX_RAND_CH_SIZE0);
        host.last_decal_size0 = evaluate_size0(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            0.0,
            rand_size,
        );
        host.last_decal_rotation = evaluate_rotation_total(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            0.0,
            req.random_seed,
            elem.view.initial_rotation,
            1.0,
        );
        let rand_color = random_table_f32(req.random_seed, FX_RAND_CH_COLOR);
        host.last_decal_color = evaluate_color_rgba(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            0.0,
            rand_color,
        )
        .map(|[r, g, b, a]| u32::from_le_bytes([b, g, r, a]));
        if let Some(visual) = elem.decal_mark_pair(req.random_seed) {
            host.last_decal_mat0_edge = visual.mark_edge_kind(0).map(str::to_owned);
            host.last_decal_mat1_edge = visual.mark_edge_kind(1).map(str::to_owned);
            host.last_decal_mat0 = visual.mark_bound_name(0).map(str::to_owned);
            host.last_decal_mat1 = visual.mark_bound_name(1).map(str::to_owned);
        }
    }

    // A Ghosts map drawn natively has no Black Ops II world to clip the mark
    // against: the native renderer clips it against its own world mesh
    // (`render_iw6::decal`; `BO2MP_IW6_DECALS=0`: no decals).
    if world.is_none() && asset_world::iw6_decals_active() {
        if let Some(spawn) = iw6_decal_spawn(host, catalog, req) {
            asset_world::push_iw6_decal(spawn);
        }
        return;
    }

    let result = host.record_impact_mark_from_decal(req.bolt);
    // bo2zm: where a decal stops on its way to a mark, on change
    // (CODLEGACY_RENDER_LOG=1 only: it changes nearly every shot).
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| codlegacy_env::var_os("CODLEGACY_RENDER_LOG").is_some()) {
        use std::sync::Mutex;
        static LAST: Mutex<Option<String>> = Mutex::new(None);
        let line = format!(
            "entered={} world={} models={} scene={} mat0={:?}/{:?} mat1={:?}/{:?}",
            result.entered,
            result.against_world,
            result.against_models,
            world.is_some(),
            host.last_decal_mat0,
            host.last_decal_mat0_edge,
            host.last_decal_mat1,
            host.last_decal_mat1_edge,
        );
        if let Ok(mut last) = LAST.lock()
            && last.as_deref() != Some(line.as_str())
        {
            diag::info!(World, "bo2zm decal: {line}");
            *last = Some(line);
        }
    }
    if result.entered && world.is_none() {
        host.note_box_surfaces_skip();
    }
    if result.against_world || result.against_models {
        if let Some(scene) = world {
            scene.finish_impact_marks(
                host,
                req.def_index,
                result.against_world,
                result.against_models,
            );
        } else {
            if result.against_world {
                host.note_world_go_skip(req.def_index);
            }
            if result.against_models {
                host.note_models_go_skip(req.def_index);
            }
        }
    } else if result.entered {
        if let Some(scene) = world {
            if scene.box_surfaces_prelude_ran(host) {
                host.note_box_surfaces_run();
            } else {
                host.note_box_surfaces_skip();
            }
        }
    }
    if !result.entered {
        host.note_impact_mark_outer_skip(req.def_index);
    }
    host.push_mark_trace(&result);
}

pub fn catalog_lookup(catalog: &FxDefinitions, index: u16) -> Option<&OwnedFxEffectDef> {
    (index != FX_CATALOG_INDEX_NONE)
        .then(|| catalog.def_at(index as usize))
        .flatten()
}

fn catalog_lookup_draw<'a>(
    catalog: &'a FxDefinitions,
    index: u16,
    index_n: &Cell<u32>,
    name_n: &Cell<u32>,
) -> Option<&'a OwnedFxEffectDef> {
    if index != FX_CATALOG_INDEX_NONE {
        index_n.set(index_n.get().saturating_add(1));
        catalog.def_at(index as usize)
    } else {
        name_n.set(name_n.get().saturating_add(1));
        None
    }
}

fn catalog_index_of(catalog: &FxDefinitions, effect: &OwnedFxEffectDef) -> u16 {
    catalog
        .index_of(effect)
        .and_then(|i| u16::try_from(i).ok())
        .unwrap_or(FX_CATALOG_INDEX_NONE)
}

struct CollideJobOut {
    handle: u16,
    result: Option<FxElemMotionResult>,
    gap: Option<u8>,
}

fn eval_pending_collide(
    catalog: &FxDefinitions,
    clip_world: Option<&SimWorld>,
    job: &PendingCollide,
) -> CollideJobOut {
    let q = job.query();
    let mut out = CollideJobOut {
        handle: job.handle,
        result: None,
        gap: None,
    };
    let Some(effect) = catalog_lookup(catalog, q.catalog_index) else {
        return out;
    };
    let Some(elem) = effect.elems.get(q.def_index as usize) else {
        return out;
    };
    if fx_iw4::elem_uses_collision(elem.view.flags) {
        let Some(world) = clip_world else {
            out.gap = Some(q.def_index);
            return out;
        };
        out.result = evaluate_elem_collide_motion(
            elem.view.flags,
            elem.view.gravity_base,
            elem.view.gravity_amplitude,
            elem.view.reflection_factor[0],
            elem.view.reflection_factor[1],
            elem.view.coll_mins,
            elem.view.coll_maxs,
            elem.view.use_item_clip != 0,
            !elem.effect_on_impact.is_absent(),
            q.elem_random_seed,
            elem.vel_graph_local.as_slice(),
            elem.vel_graph_world.as_slice(),
            q,
            |start, end, mins, maxs, mask| {
                let t = world.trace_world(start, end, mins, maxs, mask);
                FxElemTraceHit {
                    fraction: t.fraction,
                    normal: t.normal,
                    startsolid: t.startsolid != 0,
                    allsolid: t.allsolid != 0,
                }
            },
        );
        return out;
    }
    out.result = evaluate_elem_motion(
        elem.view.flags,
        elem.view.gravity_base,
        elem.view.gravity_amplitude,
        q.elem_random_seed,
        elem.vel_graph_local.as_slice(),
        elem.vel_graph_world.as_slice(),
        q,
    );
    out
}

const COLLIDE_JOBS_PER_WORKER: usize = 16;

/// `FX_ELEM_NONUNIFORM_SCALE`: the sprite's height is its own size (size1).
const FX_ELEM_NONUNIFORM_SCALE: i32 = 0x1000_0000;

fn collide_worker_count(jobs: usize) -> usize {
    static THREADS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let threads = *THREADS.get_or_init(|| {
        std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).max(1))
            .unwrap_or(1)
    });
    threads.min(jobs / COLLIDE_JOBS_PER_WORKER).max(1)
}

fn run_collide_jobs(
    catalog: &FxDefinitions,
    clip_world: Option<&SimWorld>,
    jobs: &[PendingCollide],
) -> (HashMap<u16, Option<FxElemMotionResult>>, Vec<u8>) {
    let workers = collide_worker_count(jobs.len());
    let outs = if workers <= 1 || jobs.len() == 1 {
        jobs.iter()
            .map(|job| eval_pending_collide(catalog, clip_world, job))
            .collect::<Vec<_>>()
    } else {
        let chunk_len = jobs.len().div_ceil(workers);

        ComputeTaskPool::get()
            .scope(|scope| {
                for chunk in jobs.chunks(chunk_len) {
                    scope.spawn(async move {
                        chunk
                            .iter()
                            .map(|job| eval_pending_collide(catalog, clip_world, job))
                            .collect::<Vec<_>>()
                    });
                }
            })
            .into_iter()
            .flatten()
            .collect()
    };
    let mut map = HashMap::with_capacity(outs.len());
    let mut gaps = Vec::new();
    for out in outs {
        if let Some(def_index) = out.gap {
            gaps.push(def_index);
        }
        map.insert(out.handle, out.result);
    }
    (map, gaps)
}

pub fn tick_fx_non_dependent(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    msec: i32,
    camera_origin: [f32; 3],
    clip_world: Option<&SimWorld>,
    world: Option<&dyn FxScene>,
) {
    tick_fx_pass(
        host,
        catalog,
        cache,
        msec,
        camera_origin,
        clip_world,
        world,
        true,
        true,
    )
}

pub fn tick_fx_remaining(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    msec: i32,
    camera_origin: [f32; 3],
    clip_world: Option<&SimWorld>,
    world: Option<&dyn FxScene>,
) {
    tick_fx_pass(
        host,
        catalog,
        cache,
        msec,
        camera_origin,
        clip_world,
        world,
        false,
        false,
    )
}

fn tick_fx_pass(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cache: &FxElemInfoCache,
    msec: i32,
    camera_origin: [f32; 3],
    clip_world: Option<&SimWorld>,
    world: Option<&dyn FxScene>,
    advance_frame: bool,
    non_bolted_only: bool,
) {
    host.set_msec_now(msec);
    if advance_frame {
        host.advance_frame_stamp();
    }

    let collide_gap_indices = RefCell::new(Vec::new());
    let mut emit_rand_gap_indices = Vec::new();
    fx_update(
        host,
        non_bolted_only,
        camera_origin,
        |sys, slot, prev, now, _| {
            let index = sys
                .effect_at(slot)
                .map(|e| e.catalog_index)
                .unwrap_or(FX_CATALOG_INDEX_NONE);
            let effect = catalog_lookup(catalog, index)?;
            let elems = cache.arc_for(catalog, effect);
            spawn_looping_partial(sys, slot, def_info(effect, &elems), prev, now);
            Some(effect.view.msec_looping_life)
        },
        |q| {
            let out = (|| {
                let effect = catalog_lookup(catalog, q.catalog_index)?;
                let elem = effect.elems.get(q.def_index as usize)?;
                if fx_iw4::elem_uses_collision(elem.view.flags) {
                    let Some(world) = clip_world else {
                        collide_gap_indices.borrow_mut().push(q.def_index);
                        return None;
                    };
                    return evaluate_elem_collide_motion(
                        elem.view.flags,
                        elem.view.gravity_base,
                        elem.view.gravity_amplitude,
                        elem.view.reflection_factor[0],
                        elem.view.reflection_factor[1],
                        elem.view.coll_mins,
                        elem.view.coll_maxs,
                        elem.view.use_item_clip != 0,
                        !elem.effect_on_impact.is_absent(),
                        q.elem_random_seed,
                        elem.vel_graph_local.as_slice(),
                        elem.vel_graph_world.as_slice(),
                        q,
                        |start, end, mins, maxs, mask| {
                            let t = world.trace_world(start, end, mins, maxs, mask);
                            FxElemTraceHit {
                                fraction: t.fraction,
                                normal: t.normal,
                                startsolid: t.startsolid != 0,
                                allsolid: t.allsolid != 0,
                            }
                        },
                    );
                }
                evaluate_elem_motion(
                    elem.view.flags,
                    elem.view.gravity_base,
                    elem.view.gravity_amplitude,
                    q.elem_random_seed,
                    elem.vel_graph_local.as_slice(),
                    elem.vel_graph_world.as_slice(),
                    q,
                )
            })();
            out
        },
        |q| {
            let effect = catalog_lookup(catalog, q.catalog_index)?;
            let elem = effect.elems.get(q.def_index as usize)?;
            if elem.effect_emitted.is_absent() {
                return None;
            }
            let (base, max) = fx_iw4::emit_dist_range(
                elem.view.emit_dist[0],
                elem.view.emit_dist[1],
                elem.view.emit_dist_variance[0],
                elem.view.emit_dist_variance[1],
                q.elem_random_seed,
            );

            if elem.view.emit_dist_variance[1] != 0.0 {
                emit_rand_gap_indices.push(q.def_index);
            }
            Some(fx_iw4::process_emitting_schedule(
                q.emit_residual,
                q.origin_begin,
                q.origin_end,
                q.msec_update_begin,
                q.msec_update_end,
                base,
                max,
                || 0.0,
            ))
        },
        |sys, req| {
            let Some(parent) = catalog_lookup(catalog, req.catalog_index) else {
                return false;
            };
            let Some(elem) = parent.elems.get(req.def_index as usize) else {
                return false;
            };
            let child_edge = match req.kind {
                FxChildKind::Impact => elem.effect_on_impact,
                FxChildKind::Death => elem.effect_on_death,
                FxChildKind::Emitted => elem.effect_emitted,
            };

            if child_edge.is_absent() {
                return true;
            }

            let Some(index) = child_edge.bound_index() else {
                return false;
            };
            let Some(child) = catalog.def_at(index).filter(|d| !d.elems.is_empty()) else {
                return false;
            };
            let child_name = child.name.as_str();
            let elems = cache.arc_for(catalog, child);
            match spawn_impact_or_death_effect(
                sys,
                FxPlayRequest {
                    def_name: child_name,
                    pose: FxPlayPose {
                        origin: req.origin,
                        axis: req.axis,
                        msec: req.msec,
                    },
                    wants_spotlight: false,
                    catalog_index: catalog_index_of(catalog, child),
                    def: Some(def_info(child, &elems)),
                },
            ) {
                PlayResult::PlayedReleased { .. } | PlayResult::Held { .. } => true,
                PlayResult::Failed(_) => false,
            }
        },
        |index, def_index| {
            let effect = catalog_lookup(catalog, index)?;
            cache
                .arc_for(catalog, effect)
                .get(def_index as usize)
                .copied()
        },
        |start, end, mins, maxs, mask| {
            let Some(world) = clip_world else {
                return None;
            };

            let t = world.trace_world(start, end, mins, maxs, mask);
            Some(FxTrailCollideHit {
                fraction: t.fraction,
                normal: t.normal,
                startsolid: t.startsolid != 0,
                allsolid: t.allsolid != 0,
            })
        },
        |index, def_index| {
            catalog_lookup(catalog, index)
                .and_then(|effect| effect.elems.get(def_index as usize))
                .map(|elem| (elem.vel_graph_local.clone(), elem.vel_graph_world.clone()))
                .unwrap_or_else(|| (Vec::new(), Vec::new()))
        },
        |q: FxSparkFillQuery<'_>| {
            let effect = catalog_lookup(catalog, q.catalog_index)?;
            let elem = effect.elems.get(q.def_index as usize)?;
            let rand_size = random_table_f32(q.elem_random_seed, FX_RAND_CH_SIZE0);
            let rand_scale = random_table_f32(q.elem_random_seed, FX_RAND_CH_SCALE);
            let rand_color = random_table_f32(q.elem_random_seed, FX_RAND_CH_COLOR);
            let size0 = evaluate_size0(
                elem.vis_samples.as_slice(),
                elem.view.vis_state_interval_count,
                q.norm_time,
                rand_size,
            )?;
            let scale = evaluate_scale(
                elem.vis_samples.as_slice(),
                elem.view.vis_state_interval_count,
                q.norm_time,
                rand_scale,
            )
            .unwrap_or(0.0);
            let color = evaluate_color_rgba(
                elem.vis_samples.as_slice(),
                elem.view.vis_state_interval_count,
                q.norm_time,
                rand_color,
            )
            .unwrap_or([255, 255, 255, 255]);
            Some(FxSparkFillVisual {
                size0,
                scale,
                color_rgba: color,
                spawn_angles: elem.view.spawn_angles,
                angular_velocity: elem.view.angular_velocity,
            })
        },
        |jobs| {
            if jobs.is_empty() {
                return HashMap::new();
            }
            let (map, mut gaps) = run_collide_jobs(catalog, clip_world, jobs);
            collide_gap_indices.borrow_mut().append(&mut gaps);
            map
        },
        |start, end| {
            let Some(world) = clip_world else {
                return (1.0, [0.0, 0.0, 1.0]);
            };
            let t = world.trace_world(
                start,
                end,
                fx_iw4::FX_SPARK_FOUNTAIN_TRACE_BOUNDS,
                fx_iw4::FX_SPARK_FOUNTAIN_TRACE_BOUNDS,
                fx_iw4::FX_SPARK_FOUNTAIN_TRACE_MASK,
            );
            (t.fraction, t.normal)
        },
    );
    let collide_gap_indices = collide_gap_indices.into_inner();
    for def_index in collide_gap_indices {
        host.gaps
            .raise(FxGapCause::NoWorldClipForCollide { def_index });
    }
    for def_index in emit_rand_gap_indices {
        host.gaps
            .raise(FxGapCause::EmitRandVarianceNotApplied { def_index });
    }
    drain_spawn_side_effects(host, catalog, cache, world);
}

/// `BO2MP_IW6_FX_FRAME` (on): Ghosts' elements use their own frame's axes.
pub fn iw6_frame_on() -> bool {
    // (Read once: this is asked for every element of every frame.)
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| asset_audio::iw6_rules::switch("FX_FRAME", true))
}

/// `BO2MP_IW6_FX_CULL` (on): a Ghosts element is dropped when its sphere (its
/// tail's capsule) lies wholly outside a view plane, the exe's per-element
/// draw cull (`fx_cull_elem_draw`, the same dvar as the clouds'); off: every
/// element is built and left to the GPU's clip.
pub fn iw6_cull_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| asset_audio::iw6_rules::switch("FX_CULL", true))
}

/// `BO2MP_IW6_FX_HIDE` (diagnostic): a comma list of what not to draw of
/// Ghosts' elements: `billboard`, `oriented`, `tail`, or `mat:<part>` /
/// `fx:<part>` (a part of the material's or effect's name). Empty by default.
pub(crate) fn iw6_hide() -> &'static Hide {
    static HIDE: std::sync::OnceLock<Hide> = std::sync::OnceLock::new();
    HIDE.get_or_init(|| Hide::parse(&std::env::var("BO2MP_IW6_FX_HIDE").unwrap_or_default()))
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct Hide {
    types: [bool; 3],
    mats: Vec<String>,
    fxs: Vec<String>,
}

impl Hide {
    pub(crate) fn parse(list: &str) -> Self {
        let mut h = Hide::default();
        for item in list.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()) {
            match item.as_str() {
                "billboard" => h.types[0] = true,
                "oriented" => h.types[1] = true,
                "tail" => h.types[2] = true,
                _ => {
                    if let Some(m) = item.strip_prefix("mat:") {
                        h.mats.push(m.to_owned());
                    } else if let Some(f) = item.strip_prefix("fx:") {
                        h.fxs.push(f.to_owned());
                    }
                }
            }
        }
        h
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.types == [false; 3] && self.mats.is_empty() && self.fxs.is_empty()
    }

    /// Whether the element (IW4 type, effect name, material name) is hidden.
    pub(crate) fn hides(&self, elem_type: u8, fx: &str, material: &str) -> bool {
        if self.types.get(usize::from(elem_type)).copied().unwrap_or(false) {
            return true;
        }
        let has = |parts: &[String], name: &str| {
            parts.iter().any(|p| name.len() >= p.len() && name.to_ascii_lowercase().contains(p.as_str()))
        };
        has(&self.mats, material) || has(&self.fxs, fx)
    }
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn build_fx_verts(
    host: &mut FxSystemHost,
    catalog: &FxDefinitions,
    cull: FxDrawCull<'_>,
    camera: [f32; 3],
) -> (FxGenerateVertsOut, FxVertsGaps) {
    let mut gaps = FxVertsGaps::default();
    // (This frame's distortion sprites go to the native renderer: a new list.)
    let distortion_live = asset_world::iw6_distortion_active();
    if distortion_live {
        asset_world::clear_iw6_distortion();
    }
    let catalog_index_n = Cell::new(0u32);
    let catalog_name_n = Cell::new(0u32);
    let vis_blocker_adds = RefCell::new(Vec::new());
    // Most adjacent particles use the same definition/material. Share their
    // diagnostic names instead of allocating two strings per generated sprite.
    // The cache lives only as long as this generation call.
    let mut sprite_names: Option<(Arc<str>, Arc<str>)> = None;
    // Ghosts effect materials' shading by material index (read once a call).
    let mut iw6_material_fx: HashMap<usize, Option<fx_iw4::iw6::material::MaterialFx>> = HashMap::new();
    let mut on_elem = |ctx: FxDrawElemContext<'_>| -> Option<FxSpriteInstance> {
        let Some(effect) = catalog_lookup_draw(
            catalog,
            ctx.catalog_index,
            &catalog_index_n,
            &catalog_name_n,
        ) else {
            gaps.no_def = gaps.no_def.saturating_add(1);
            return None;
        };
        let Some(elem) = effect.elems.get(ctx.def_index as usize) else {
            gaps.no_elem = gaps.no_elem.saturating_add(1);
            return None;
        };
        let Some((material_index, material_name)) = elem.primary_material() else {
            gaps.no_material_visual = gaps.no_material_visual.saturating_add(1);
            return None;
        };
        let to_eye = [
            ctx.origin[0] - camera[0],
            ctx.origin[1] - camera[1],
            ctx.origin[2] - camera[2],
        ];
        let eye_dist = (to_eye[0] * to_eye[0] + to_eye[1] * to_eye[1] + to_eye[2] * to_eye[2]).sqrt();
        // bo2mp: Modern Warfare Remastered's elements by the game's own rules
        // (`crate::h1_rules`: its seed, float colour and emissive, sizes,
        // rotation, fades and lit colour); the converted path otherwise.
        let h1_state = match elem.h1.as_deref().filter(|_| crate::h1_rules::on()) {
            Some(src) => match crate::h1_rules::sprite_state(src, &ctx, eye_dist) {
                Some(st) => Some(st),
                None => {
                    gaps.h1_not_drawn = gaps.h1_not_drawn.saturating_add(1);
                    return None;
                }
            },
            None => None,
        };
        // bo2mp: Ghosts' elements by Ghosts' rules (`crate::iw6_rules`: its
        // seed, colour and unlit HDR scale, sizes, rotation, fades, and the
        // lit blend with `BO2MP_IW6_FX_LIT=1`); the converted path otherwise.
        let iw6_state = match elem.iw6.as_deref().filter(|_| h1_state.is_none() && crate::iw6_rules::on()) {
            Some(src) => match crate::iw6_rules::sprite_state(src, &ctx, eye_dist, crate::iw6_rules::lit_on()) {
                Some(st) => Some(st),
                None => {
                    gaps.iw6_not_drawn = gaps.iw6_not_drawn.saturating_add(1);
                    return None;
                }
            },
            None => None,
        };
        let (size0, size1, mut color, vis_alpha) = if let Some(st) = h1_state {
            gaps.h1_drawn = gaps.h1_drawn.saturating_add(1);
            (st.size0, st.size1, st.color, st.vis_alpha)
        } else if let Some(st) = iw6_state {
            gaps.iw6_drawn = gaps.iw6_drawn.saturating_add(1);
            (st.size0, st.size1, st.color, st.vis_alpha)
        } else {
        let rand_size = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SIZE0);
        let rand_color = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_COLOR);
        let Some(size0) = evaluate_size0(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        ) else {
            gaps.no_size0_sample = gaps.no_size0_sample.saturating_add(1);
            return None;
        };
        if size0 <= 0.0 {
            gaps.size0_not_positive = gaps.size0_not_positive.saturating_add(1);
            return None;
        }

        let size1 = if ctx.elem_type == 2 {
            let Some(s1) = evaluate_size1(
                elem.vis_samples.as_slice(),
                elem.view.vis_state_interval_count,
                ctx.norm_time,
                rand_size,
            ) else {
                gaps.no_size1_sample = gaps.no_size1_sample.saturating_add(1);
                return None;
            };
            if s1 <= 0.0 {
                gaps.size1_not_positive = gaps.size1_not_positive.saturating_add(1);
                return None;
            }
            s1
        } else if ctx.flags & FX_ELEM_NONUNIFORM_SCALE != 0 {
            // bo2zm: a sprite with non-uniform scale is size0 wide and size1
            // tall (Black Ops II's wall-buy chalk is 24 x 7.25, the glow
            // behind it 75 x 27); without a height it stays square.
            evaluate_size1(
                elem.vis_samples.as_slice(),
                elem.view.vis_state_interval_count,
                ctx.norm_time,
                rand_size,
            )
            .filter(|s1| *s1 > 0.0)
            .unwrap_or(0.0)
        } else {
            0.0
        };
        let (mut color, color_missing) = match evaluate_color_rgba(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_color,
        ) {
            Some(c) => (c, false),
            None => ([255, 255, 255, 255], true),
        };
        if color_missing {
            gaps.color_fallback = gaps.color_fallback.saturating_add(1);
        }
        let vis_alpha = color[3];
        // bo2zm: the engine's distance fade (`fadeInRange`, `fadeOutRange`,
        // as its sight blockers read it): Black Ops II keeps its muzzle smoke,
        // world-sized flashes and explosion parts clear of the eye, and fades
        // far fires out. A sprite it fades away entirely is not drawn.
        if let Some(fade) = fx_iw4::evaluate_distance_fade(
            eye_dist,
            elem.view.fade_in_range[0],
            elem.view.fade_in_range[1],
            elem.view.fade_out_range[0],
            elem.view.fade_out_range[1],
        ) {
            let a = (u32::from(color[3]) * fade.min(255)) / 255;
            if a == 0 {
                return None;
            }
            color[3] = a as u8;
        }
        (size0, size1, color, vis_alpha)
        };

        // bo2mp: a Ghosts flare (type 4) is placed in screen space by the
        // exe's handler (`crate::iw6_flare`): its centre, size, turn and alpha.
        let flare = match elem.iw6.as_deref().and_then(|s| s.flare.as_ref()) {
            Some(def) if crate::iw6_flare::on() => {
                let Some(view) = asset_world::iw6_fx_view() else { return None };
                let placed = crate::iw6_flare::place(def, &view, ctx.origin, ctx.frame_axis, [size0, size1], f32::from(color[3]) / 255.0, 0.0);
                match placed {
                    Some(p) => Some(p),
                    None => {
                        gaps.iw6_not_drawn = gaps.iw6_not_drawn.saturating_add(1);
                        return None;
                    }
                }
            }
            _ => None,
        };
        let (size0, size1) = flare.map_or((size0, size1), |p| (p.size0, p.size1));
        if let Some(p) = flare {
            color[3] = (p.alpha * 255.0).round() as u8;
        }

        // bo2mp: an MWR / HMW first-person flash sprite held to a small part
        // of the screen (`fx_iw4::FX_VIEW_FLASH_SPRITE_TAN_MAX`: the red mist).
        // (The name test first: it allocates nothing.)
        let flash = fx_iw4::fx_is_muzzle_flash_name(ctx.def_name)
            || (effect.namespace == asset_core::AssetNamespace::H1
                && crate::light_units::is_weapon_flash_fx(ctx.def_name));
        let k = if flash && effect.namespace == asset_core::AssetNamespace::H1 {
            fx_iw4::fx_view_flash_sprite_scale(size0, eye_dist)
        } else {
            1.0
        };
        // bo2mp: a camera-lens overlay of an MWR / HMW gun flash (the MORS's
        // dirty lens and line flare) is not drawn: held small, its picture
        // is a hard red-orange square in the view
        // (`fx_iw4::fx_is_lens_overlay_material`).
        if flash
            && effect.namespace == asset_core::AssetNamespace::H1
            && fx_iw4::fx_is_lens_overlay_material(material_name)
        {
            crate::light_units::log_flash_sprite(effect.namespace, material_name, || {
                format!(
                    "{} / {material_name} half {size0:.1} at {eye_dist:.0}: lens overlay, not drawn",
                    ctx.def_name
                )
            });
            return None;
        }
        if flash {
            crate::light_units::log_flash_sprite(effect.namespace, material_name, || {
                format!(
                    "{} / {material_name} half {size0:.1} -> {:.1} at {eye_dist:.0} rgba {color:?} drawn",
                    ctx.def_name,
                    size0 * k
                )
            });
        }
        let (size0, size1) = (size0 * k, size1 * k);

        // (An MWR element's colour is lit by the game's rule already, as a
        // Ghosts element's under its lit blend.)
        if h1_state.is_none() && !iw6_state.is_some_and(|st| st.lit) {
            color = apply_elem_lighting(
                color,
                elem.view.lighting_frac,
                ctx.packed_lighting,
                ctx.packed_lighting_src,
                ctx.def_index,
                &mut gaps,
            );
        }

        // bo2mp: a Ghosts element lives in its own frame (`BO2MP_IW6_FX_FRAME`,
        // `docs/exe_notes/IW6_FX_EXE.md`): its tail velocity and oriented
        // sprite use that frame's axes (for an offset element the frame built
        // from the spawn offset: a burst's tails fly radially), and an
        // oriented sprite spans frame axes 1 and 2 with no random angles.
        let iw6_frame = elem.iw6.is_some() && iw6_frame_on();
        let vel_axis = if iw6_frame { ctx.frame_axis } else { ctx.axis };
        let axis = if iw6_frame && ctx.elem_type == 1 {
            // The exe's oriented sprite spans the frame's axes 1 and 2 (the
            // quad builder is handed the axes at +0x40 and +0x4c of the draw
            // state; the one at +0x34, axis 0, is its normal), as
            // `sprite_transform` does (right = axis 1, up = axis 2).
            ctx.frame_axis
        } else if ctx.elem_type == 1 {
            get_elem_angles_axis(
                elem.view.spawn_angles,
                elem.view.angular_velocity,
                ctx.elem_random_seed,
                ctx.age_msec as f32,
                ctx.axis,
            )
        } else {
            ctx.axis
        };

        let vel_dir = if ctx.elem_type == 2 {
            let vel = get_velocity_at_time(
                ctx.flags,
                ctx.base_vel,
                ctx.age_msec as f32,
                ctx.life_msec.max(1) as f32,
                elem.vel_graph_local.as_slice(),
                elem.vel_graph_world.as_slice(),
                vel_axis,
                ctx.elem_random_seed,
            );
            vec3_normalize(vel)
        } else {
            [0.0; 3]
        };
        // bo2mp: the exe's per-element draw cull (`fx_cull_elem_draw`; the
        // handlers test the element's sphere, a tail's capsule, against the
        // view planes before building any vertex) and the diagnostic hide
        // list. Ghosts' elements only.
        if elem.iw6.is_some() {
            if iw6_cull_on() && !cull.planes.is_empty() && flare.is_none() {
                let out = if ctx.elem_type == 2 {
                    let centre = [
                        ctx.origin[0] - size1 * vel_dir[0],
                        ctx.origin[1] - size1 * vel_dir[1],
                        ctx.origin[2] - size1 * vel_dir[2],
                    ];
                    fx_iw4::cull_tail(cull.elem_draw, cull.planes, ctx.flags, centre, vel_dir, size1, size0)
                } else {
                    fx_iw4::cull_elem_sprite(cull.elem_draw, cull.planes, ctx.flags, ctx.origin, size0, size1)
                };
                if out {
                    gaps.iw6_culled = gaps.iw6_culled.saturating_add(1);
                    return None;
                }
            }
            let hide = iw6_hide();
            if !hide.is_empty() && hide.hides(ctx.elem_type, ctx.def_name, material_name) {
                return None;
            }
        }
        // bo2mp: a Ghosts effect material's own shader work
        // (`fx_iw4::iw6::material`, `BO2MP_IW6_FX_MATERIAL`): the view-angle
        // falloff of `*falloff*` techniques (godray beams and window shafts
        // fade edge on), the near ramp of `zfeather`, the eye offset.
        let mut sprite_origin = flare.map_or(ctx.origin, |p| p.origin);
        if elem.iw6.is_some() && flare.is_none() {
            let mfx = *iw6_material_fx
                .entry(material_index)
                .or_insert_with(|| asset_game::iw6_fx_material(material_name));
            if let Some(m) = mfx {
                if m.eye_offset != 0.0 && eye_dist > 1e-3 {
                    let k = m.eye_offset.min(eye_dist * 0.5) / eye_dist;
                    for i in 0..3 {
                        sprite_origin[i] -= to_eye[i] * k;
                    }
                }
                let near = fx_iw4::iw6::material::near_ramp(eye_dist, m.near_scale);
                let mut rgb_k = [1.0f32; 3];
                let mut a_k = near;
                if let Some(f) = &m.falloff {
                    // The quad's normal as `sprite_transform` builds it.
                    let normal = match ctx.elem_type {
                        1 => Some(vec3_normalize(cross3(axis[1], axis[2]))),
                        2 => {
                            let centre = [
                                sprite_origin[0] - size1 * vel_dir[0],
                                sprite_origin[1] - size1 * vel_dir[1],
                                sprite_origin[2] - size1 * vel_dir[2],
                            ];
                            fx_iw4::tail_sprite_axes(vel_dir, camera, centre).map(|a| a[2])
                        }
                        // (A billboard faces the eye: N . V = 1.)
                        _ => None,
                    };
                    let cos = match normal {
                        Some(n) => {
                            let v = vec3_normalize([
                                sprite_origin[0] - camera[0],
                                sprite_origin[1] - camera[1],
                                sprite_origin[2] - camera[2],
                            ]);
                            n[0] * v[0] + n[1] * v[1] + n[2] * v[2]
                        }
                        None => 1.0,
                    };
                    let (rgb, a) = fx_iw4::iw6::material::falloff_factors(cos, f);
                    rgb_k = rgb;
                    a_k *= a;
                }
                for i in 0..3 {
                    color[i] = (f32::from(color[i]) * rgb_k[i].clamp(0.0, 1.0)).round() as u8;
                }
                color[3] = (f32::from(color[3]) * a_k.clamp(0.0, 1.0)).round() as u8;
                gaps.iw6_material = gaps.iw6_material.saturating_add(1);
                if color[3] == 0 {
                    return None;
                }
            }
        }
        let rotation_rad = match flare.map(|p| p.rotation).or(h1_state.map(|st| st.rotation)).or(iw6_state.map(|st| st.rotation)) {
            Some(r) => r,
            None => evaluate_rotation_total(
                elem.vis_samples.as_slice(),
                elem.view.vis_state_interval_count,
                ctx.norm_time,
                ctx.elem_random_seed,
                elem.view.initial_rotation,
                ctx.life_msec as f32,
            )
            .unwrap_or(0.0),
        };
        let atlas = if elem.raw.len() >= FX_ELEM_ATLAS_OFF + FX_ELEM_ATLAS_SIZE {
            sprite_atlas_uv(
                &elem.raw[FX_ELEM_ATLAS_OFF..FX_ELEM_ATLAS_OFF + FX_ELEM_ATLAS_SIZE],
                ctx.elem_random_seed,
                ctx.sequence,
                ctx.age_msec,
                ctx.norm_time,
            )
        } else {
            fx_iw4::FxSpriteAtlasUv::FULL
        };

        // bo2mp: a Ghosts distortion sprite (heat haze, a shockwave) is not
        // drawn by this pass: the native renderer draws it over a copy of the
        // scene (`render_iw6::distort`), so its quad goes there, and only
        // when it is in view (the culls above). Never a flat sprite.
        if elem.iw6.is_some() && asset_game::iw6_fx_distortion(material_name) {
            if distortion_live
                && let Some(view) = asset_world::iw6_fx_view()
                && let Some(id) = asset_world::iw6_decal_material_id(material_name)
            {
                let quad = distortion_quad(&view, sprite_origin, [size0, size1], ctx.elem_type, axis, rotation_rad, color, id);
                asset_world::push_iw6_distortion(quad);
            }
            return None;
        }
        vis_blocker_adds.borrow_mut().push((
            elem.view.flags,
            ctx.origin,
            size0,
            vis_alpha,
            elem.view.fade_in_range,
            elem.view.fade_out_range,
        ));
        if !sprite_names.as_ref().is_some_and(|(def, material)| {
            def.as_ref() == ctx.def_name && material.as_ref() == material_name
        }) {
            sprite_names = Some((Arc::from(ctx.def_name), Arc::from(material_name)));
        }
        let (def_name, material_name) = sprite_names.as_ref().unwrap();
        Some(FxSpriteInstance {
            origin: sprite_origin,
            size0,
            size1,
            color_rgba: color,
            elem_type: ctx.elem_type,
            def_name: Arc::clone(def_name),
            catalog_index: ctx.catalog_index,
            def_index: ctx.def_index,
            material_name: Arc::clone(material_name),
            material_index: Some(material_index),
            axis,
            rotation_rad,
            vel_dir,
            atlas,
            flags: ctx.flags,
            lighting_sample: ctx.packed_lighting,
            lighting_frac: elem.view.lighting_frac,
            packed_lighting_src: ctx.packed_lighting_src,
        })
    };

    let mut on_trail_def = |ctx: FxDrawTrailContext<'_>, def_index: u8| -> Option<FxTrailDrawDef> {
        let effect = catalog_lookup_draw(
            catalog,
            ctx.catalog_index,
            &catalog_index_n,
            &catalog_name_n,
        )?;
        let elem = effect.elems.get(def_index as usize)?;
        let trail = elem.trail_def.as_ref()?;
        if trail.verts.is_empty() || trail.inds.len() < 2 || trail.repeat_dist == 0 {
            return None;
        }
        let (material_index, material_name) = elem.primary_material()?;
        Some(FxTrailDrawDef {
            visual_count: elem.view.visual_count,
            flags: elem.view.flags,
            life_base: elem.view.life_span_msec_base,
            life_amp: elem.view.life_span_msec_amplitude,
            scroll_time_msec: trail.scroll_time_msec,
            repeat_dist: trail.repeat_dist,
            verts: trail.verts.clone(),
            inds: trail.inds.clone(),
            material_name: material_name.to_owned(),
            material_index: Some(material_index),
        })
    };
    let mut on_trail_sample = |ctx: FxDrawTrailSampleContext<'_>| -> Option<FxTrailSampleVisual> {
        let effect = catalog_lookup_draw(
            catalog,
            ctx.catalog_index,
            &catalog_index_n,
            &catalog_name_n,
        )?;
        let elem = effect.elems.get(ctx.def_index as usize)?;
        let rand_size = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SIZE0);
        let rand_color = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_COLOR);
        let size0 = evaluate_size0(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        )?;
        if size0 <= 0.0 {
            return None;
        }
        let size1 = evaluate_size1(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        )
        .unwrap_or(size0);
        let color = evaluate_color_rgba(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_color,
        )
        .unwrap_or([255, 255, 255, 255]);
        let rotation = evaluate_rotation_total(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            ctx.elem_random_seed,
            elem.view.initial_rotation,
            ctx.life_msec as f32,
        )
        .unwrap_or(0.0);

        vis_blocker_adds.borrow_mut().push((
            elem.view.flags,
            ctx.sample_origin,
            size0,
            color[3],
            elem.view.fade_in_range,
            elem.view.fade_out_range,
        ));
        Some(FxTrailSampleVisual {
            size: [size0, size1],
            color_rgba: color,
            rotation,
        })
    };

    let mut on_spark_size1 = |ctx: FxSparkDrawQuery<'_>| -> Option<f32> {
        let effect = catalog_lookup_draw(
            catalog,
            ctx.catalog_index,
            &catalog_index_n,
            &catalog_name_n,
        )?;
        let elem = effect.elems.get(ctx.def_index as usize)?;
        let rand_size = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SIZE0);
        let size1 = evaluate_size1(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        )?;
        (size1 >= 0.0).then_some(size1)
    };
    let mut on_cloud = |ctx: FxDrawElemContext<'_>| -> Option<FxCloudInstance> {
        let effect = catalog_lookup_draw(
            catalog,
            ctx.catalog_index,
            &catalog_index_n,
            &catalog_name_n,
        )?;
        let elem = effect.elems.get(ctx.def_index as usize)?;
        let rand_size = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SIZE0);
        let rand_scale = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SCALE);
        let rand_color = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_COLOR);
        let size0 = evaluate_size0(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        )?;
        let size1 = evaluate_size1(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        )
        .unwrap_or(size0);
        let scale = evaluate_scale(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_scale,
        )?;
        if cull_cloud(
            cull.elem_draw,
            cull.planes,
            cull.planes.len() as u32,
            ctx.flags,
            ctx.origin,
            size0,
            size1,
            scale,
        ) {
            return None;
        }
        let (color, _) = match evaluate_color_rgba(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_color,
        ) {
            Some(c) => (c, false),
            None => ([255, 255, 255, 255], true),
        };
        // bo2zm: lit by its lighting fraction of the light where it is, as
        // every element's colour is (Black Ops II's ash clouds use it).
        let color = apply_elem_lighting(
            color,
            elem.view.lighting_frac,
            ctx.packed_lighting,
            ctx.packed_lighting_src,
            ctx.def_index,
            &mut FxVertsGaps::default(),
        );
        let rot_t = clamp_elem_rotation_time(
            ctx.age_msec as f32,
            ctx.life_msec as f32,
            ctx.at_rest_fraction,
        );
        let axis = get_elem_angles_axis(
            elem.view.spawn_angles,
            elem.view.angular_velocity,
            ctx.elem_random_seed,
            rot_t,
            ctx.axis,
        );
        let vel = get_velocity_at_time(
            ctx.flags,
            ctx.base_vel,
            ctx.age_msec as f32,
            ctx.life_msec.max(1) as f32,
            elem.vel_graph_local.as_slice(),
            elem.vel_graph_world.as_slice(),
            ctx.axis,
            ctx.elem_random_seed,
        );
        // bo2zm: a BO2 cloud's own particle count, its random extra on the
        // scale's channel (a bigger cloud, more particles).
        let particles = elem.cloud_density.map(|[base, extra]| {
            let n = base as f32 + extra.max(0) as f32 * rand_scale;
            n.clamp(1.0, fx_iw4::FX_PARTICLE_CLOUD_TEMPLATE_CELLS as f32) as u32
        });
        Some(FxCloudInstance {
            particles,
            def_name: ctx.def_name.to_owned(),
            catalog_index: ctx.catalog_index,
            def_index: ctx.def_index,
            cloud: build_cloud(
                ctx.origin,
                axis,
                size0,
                size1,
                scale,
                color,
                ctx.flags,
                vel,
                ctx.age_msec as f32,
            ),
        })
    };
    // bo2zm: Black Ops II's effect light colour (`R_AddOmniLightToScene`):
    // the element's colour from sRGB to linear, times its alpha times 32, in
    // the world's light units (its sun is about 15 before exposure).
    fn t6_elem_light_color_bgr(color_rgba: [u8; 4]) -> [f32; 3] {
        let lin = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let s = color_rgba[3] as f32 / 255.0 * 32.0;
        [
            lin(color_rgba[2]) * s,
            lin(color_rgba[1]) * s,
            lin(color_rgba[0]) * s,
        ]
    }
    let t7_scene_lights: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::new());
    // bo2mp: each light in the map's units, held under its caps
    // (`crate::light_units`).
    let map_ns = catalog.map_namespace();
    let map_units = crate::light_units::map_light_units(assets::map_game(), map_ns);
    let mut on_light = |ctx: FxDrawElemContext<'_>| -> Option<FxElemLightInstance> {
        let effect = catalog_lookup_draw(
            catalog,
            ctx.catalog_index,
            &catalog_index_n,
            &catalog_name_n,
        )?;
        let elem = effect.elems.get(ctx.def_index as usize)?;
        // bo2mp: a Black Ops III light element adds the exe's scene light
        // (`fastfile_t7::scene_light`) for the native renderer, and the
        // same light to Black Ops II's list.
        if let Some(t7) = catalog.t7_lights.get(&(effect.name.to_ascii_lowercase(), ctx.def_index)) {
            use fastfile_t7::scene_light as sl;
            let (floor, frac) = fx_iw4::setup_visual_sample_point(t7.vis_intervals, ctx.norm_time);
            let seed = ctx.elem_random_seed;
            let word = |w: usize, ch: u32| sl::sample_word(&t7.vis_samples, floor, frac, w, random_table_f32(seed, sl::RAND_CHANNEL + ch));
            let colour = sl::sample_colour(&t7.vis_samples, floor, frac, random_table_f32(seed, FX_RAND_CH_COLOR))?;
            let rot_t = clamp_elem_rotation_time(ctx.age_msec as f32, ctx.life_msec as f32, ctx.at_rest_fraction);
            let axis = get_elem_angles_axis(elem.view.spawn_angles, elem.view.angular_velocity, seed, rot_t, ctx.axis);
            let light = sl::elem_light(
                &t7.description,
                &sl::ElemLight { origin: ctx.origin, axis, colour, intensity: word(0x18, 1)?, size: word(0x1c, 0)?, angle: word(0x20, 3)? },
            )?;
            let rd = |at: usize| f32::from_le_bytes(light[at..at + 4].try_into().unwrap());
            let radius = rd(0x14c);
            if cull_elem_light(cull.elem_draw, cull.planes, cull.planes.len() as u32, ctx.flags, ctx.origin, radius) {
                return None;
            }
            let (color_bgr, radius) = crate::light_units::normalise_effect_light(
                effect.namespace,
                map_ns,
                map_units,
                ctx.def_name,
                false,
                [rd(0x60), rd(0x5c), rd(0x58)],
                radius,
            )?;
            let mut list = t7_scene_lights.borrow_mut();
            if list.len() >= sl::MAX {
                return None;
            }
            list.push(light);
            return Some(FxElemLightInstance {
                def_name: ctx.def_name.to_owned(),
                def_index: ctx.def_index,
                is_spot: false,
                origin: ctx.origin,
                radius,
                color_bgr,
                axis,
            });
        }
        // bo2mp: a Ghosts light element by Ghosts' own rule (its seed, the
        // sample's colour times scale, the size as radius; `BO2MP_IW6_FX_LIGHTS`).
        if let Some(src) = elem.iw6.as_deref().filter(|_| crate::iw6_rules::on() && crate::iw6_lights::on()) {
            use fx_iw4::iw6 as g;
            let seed = g::elem_seed(ctx.sequence, ctx.effect_seed, src.rules.random_seed, ctx.msec_begin);
            let (_, norm) = g::draw_age(ctx.age_msec, ctx.life_msec.max(1));
            let l = g::light_state(&src.rules, &src.vis_samples, norm, seed, ctx.life_msec as f32)?;
            if cull_elem_light(cull.elem_draw, cull.planes, cull.planes.len() as u32, ctx.flags, ctx.origin, l.radius) {
                return None;
            }
            let rot_t = clamp_elem_rotation_time(ctx.age_msec as f32, ctx.life_msec as f32, ctx.at_rest_fraction);
            let axis = get_elem_angles_axis(elem.view.spawn_angles, elem.view.angular_velocity, ctx.elem_random_seed, rot_t, ctx.axis);
            let is_spot = ctx.elem_type == FxElemType::SpotLight as u8;
            let (color_bgr, radius) = crate::light_units::normalise_effect_light(
                effect.namespace,
                map_ns,
                map_units,
                ctx.def_name,
                is_spot,
                [l.rgb[2], l.rgb[1], l.rgb[0]],
                l.radius,
            )?;
            return Some(FxElemLightInstance {
                def_name: ctx.def_name.to_owned(),
                def_index: ctx.def_index,
                is_spot,
                origin: ctx.origin,
                radius,
                color_bgr,
                axis,
            });
        }
        let rand_size = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SIZE0);
        let rand_scale = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SCALE);
        let rand_color = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_COLOR);
        let size0 = evaluate_size0(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_size,
        )?;
        if size0 <= 0.0 {
            return None;
        }
        let scale = evaluate_scale(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_scale,
        )?;
        if cull_elem_light(
            cull.elem_draw,
            cull.planes,
            cull.planes.len() as u32,
            ctx.flags,
            ctx.origin,
            size0,
        ) {
            return None;
        }
        let color = evaluate_color_rgba(
            elem.vis_samples.as_slice(),
            elem.view.vis_state_interval_count,
            ctx.norm_time,
            rand_color,
        )?;
        let rot_t = clamp_elem_rotation_time(
            ctx.age_msec as f32,
            ctx.life_msec as f32,
            ctx.at_rest_fraction,
        );
        let axis = get_elem_angles_axis(
            elem.view.spawn_angles,
            elem.view.angular_velocity,
            ctx.elem_random_seed,
            rot_t,
            ctx.axis,
        );
        let is_spot = ctx.elem_type == FxElemType::SpotLight as u8;
        // bo2zm: a Black Ops II light element leaves its scale at 0: its
        // brightness is its colour and alpha.
        let color_bgr = if effect.namespace == asset_core::AssetNamespace::T6 {
            t6_elem_light_color_bgr(color)
        } else {
            elem_light_color_bgr(color, scale)
        };
        let (color_bgr, radius) = crate::light_units::normalise_effect_light(
            effect.namespace,
            map_ns,
            map_units,
            ctx.def_name,
            is_spot,
            color_bgr,
            size0,
        )?;
        Some(FxElemLightInstance {
            def_name: ctx.def_name.to_owned(),
            def_index: ctx.def_index,
            is_spot,
            origin: ctx.origin,
            radius,
            color_bgr,
            axis,
        })
    };
    let mut on_model = |ctx: FxDrawElemContext<'_>| {
        evaluate_fx_model_instance(catalog, ctx, &catalog_index_n, &catalog_name_n)
    };
    let out = generate_verts_with_trails(
        host,
        &mut on_elem,
        &mut on_trail_def,
        &mut on_trail_sample,
        &mut on_spark_size1,
        &mut on_cloud,
        &mut on_light,
        &mut on_model,
        camera,
    );
    if !catalog.t7_lights.is_empty() {
        fastfile_t7::scene_light::publish(t7_scene_lights.into_inner());
    }
    for (flags, origin, size0, alpha, fade_in, fade_out) in vis_blocker_adds.into_inner() {
        vis_blocker_add_prepared(
            &mut host.vis_blocker_read,
            flags,
            origin,
            size0,
            alpha,
            fade_in,
            fade_out,
            camera,
        );
    }
    gaps.catalog_index_n = catalog_index_n.get();
    gaps.catalog_name_n = catalog_name_n.get();
    (out, gaps)
}

fn evaluate_fx_model_instance(
    catalog: &FxDefinitions,
    ctx: FxDrawElemContext<'_>,
    catalog_index_n: &Cell<u32>,
    catalog_name_n: &Cell<u32>,
) -> Option<FxModelInstance> {
    let effect = catalog_lookup_draw(catalog, ctx.catalog_index, catalog_index_n, catalog_name_n)?;
    let elem = effect.elems.get(ctx.def_index as usize)?;
    let model_index = elem.model_edge(ctx.elem_random_seed)?.bound_index()?;
    let rand_scale = random_table_f32(ctx.elem_random_seed, FX_RAND_CH_SCALE);
    let scale = evaluate_scale(
        elem.vis_samples.as_slice(),
        elem.view.vis_state_interval_count,
        ctx.norm_time,
        rand_scale,
    )?;
    if scale == 0.0 {
        return None;
    }
    let rot_t = clamp_elem_rotation_time(
        ctx.age_msec as f32,
        ctx.life_msec as f32,
        ctx.at_rest_fraction,
    );
    let axis = get_elem_angles_axis(
        elem.view.spawn_angles,
        elem.view.angular_velocity,
        ctx.elem_random_seed,
        rot_t,
        ctx.axis,
    );
    Some(FxModelInstance {
        def_name: ctx.def_name.to_owned(),
        def_index: ctx.def_index,
        model_index,
        elem_handle: ctx.elem_handle,
        origin: ctx.origin,
        axis,
        scale,
        flags: ctx.flags,
    })
}

pub(crate) fn owned_elem_infos(effect: &OwnedFxEffectDef) -> Vec<FxElemDefInfo> {
    effect
        .elems
        .iter()
        .map(|e| FxElemDefInfo {
            elem_type: e.view.elem_type,
            spawn_a: e.view.spawn_a,
            spawn_b: e.view.spawn_b,
            delay_base: e.view.spawn_delay_msec_base,
            delay_amp: e.view.spawn_delay_msec_amplitude,
            life_base: e.view.life_span_msec_base,
            life_amp: e.view.life_span_msec_amplitude,
            flags: e.view.flags,
            visual_count: e.view.visual_count,
            vis_state_interval_count: e.view.vis_state_interval_count,
            spawn_range_base: e.view.spawn_range_base,
            spawn_range_amp: e.view.spawn_range_amplitude,
            spawn_origin: e.view.spawn_origin,
            spawn_offset_radius_base: e.view.spawn_offset_radius_base,
            spawn_offset_radius_amp: e.view.spawn_offset_radius_amplitude,
            spawn_offset_height_base: e.view.spawn_offset_height_base,
            spawn_offset_height_amp: e.view.spawn_offset_height_amplitude,
            has_effect_on_impact: !e.effect_on_impact.is_absent(),
            has_effect_on_death: !e.effect_on_death.is_absent(),
            has_effect_emitted: !e.effect_emitted.is_absent(),
            sort_order: e.view.sort_order,
            spark_count: e.spark_fountain_def.map(|d| d.spark_count).unwrap_or(0),
            spark_vel_min: e.spark_fountain_def.map(|d| d.vel_min).unwrap_or(0.0),
            spark_vel_max: e.spark_fountain_def.map(|d| d.vel_max).unwrap_or(0.0),
            spark_vel_cone_frac: e.spark_fountain_def.map(|d| d.vel_cone_frac).unwrap_or(0.0),
            spark_gravity: e.spark_fountain_def.map(|d| d.gravity).unwrap_or(0.0),
            spark_length: e.spark_fountain_def.map(|d| d.spark_length).unwrap_or(0.0),
            spark_loop_time: e.spark_fountain_def.map(|d| d.loop_time).unwrap_or(0.0),
            spark_boost_time: e.spark_fountain_def.map(|d| d.boost_time).unwrap_or(0.0),
            spark_boost_factor: e.spark_fountain_def.map(|d| d.boost_factor).unwrap_or(0.0),
            spark_bounce_frac: e.spark_fountain_def.map(|d| d.bounce_frac).unwrap_or(0.0),
            spark_bounce_rand: e.spark_fountain_def.map(|d| d.bounce_rand).unwrap_or(0.0),
            inv_split_dist: e
                .trail_def
                .as_ref()
                .map(|d| d.inv_split_dist)
                .unwrap_or(0.0),
            inv_split_arc_dist: e
                .trail_def
                .as_ref()
                .map(|d| d.inv_split_arc_dist)
                .unwrap_or(0.0),
            inv_split_time: e
                .trail_def
                .as_ref()
                .map(|d| d.inv_split_time)
                .unwrap_or(0.0),
            gravity_base: e.view.gravity_base,
            gravity_amp: e.view.gravity_amplitude,
            reflection_base: e.view.reflection_factor[0],
            reflection_amp: e.view.reflection_factor[1],
            coll_mins: e.view.coll_mins,
            coll_maxs: e.view.coll_maxs,
            use_item_clip: e.view.use_item_clip,
            spawn_angles: e.view.spawn_angles,
            angular_velocity: e.view.angular_velocity,
            // bo2mp: a Ghosts element spawns by Ghosts' draws
            // (`BO2MP_IW6_FX_RULES`).
            iw6_seed: e
                .iw6
                .as_ref()
                .filter(|_| crate::iw6_rules::on())
                .map(|s| s.rules.random_seed),
            iw6_draws: e.iw6.is_some() && asset_game::iw6_fx_draws_on(),
        })
        .collect()
}

/// A placed effect's axis rows (forward, left, up) from its angles (pitch,
/// yaw, roll; degrees), as the engine's `AnglesToAxis` builds them for the
/// effect the createfx script plays with the entity's forward and up. The
/// roll counts: an effect's local velocity along its up (or left) follows it.
/// (Taking the forward alone and deriving the rest, as an impact's normal
/// does, turned every local velocity not along the forward: a horizontal
/// placement's rising smoke went sideways, or down.)
pub fn axis_from_angles_deg(angles_deg: [f32; 3]) -> [[f32; 3]; 3] {
    fx_iw4::angles_to_axis_radians(angles_deg.map(f32::to_radians))
}

#[cfg(test)]
mod axis_tests {
    use super::axis_from_angles_deg;

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
    }

    /// The world velocity of a local one (units a second) in a placement's
    /// axis, as `fx_iw4::get_velocity_at_time` takes it.
    fn world(angles: [f32; 3], local: [f32; 3]) -> [f32; 3] {
        let per_ms = local.map(|v| v / 1000.0);
        let one = fx_iw4::FxElemVec3Range { base: per_ms, amplitude: [0.0; 3] };
        fx_iw4::get_velocity_at_time(
            fx_iw4::FX_ELEM_VEL_LOCAL,
            [0.0; 3],
            0.0,
            1000.0,
            &[one, one],
            &[],
            axis_from_angles_deg(angles),
            0,
        )
    }

    #[test]
    fn a_vertical_placement_carries_the_forward_velocity_up() {
        // Pitch 270 (-90): the forward axis points up; smoke authored along
        // the forward rises.
        assert!(near(world([270.0, 33.0, 0.0], [30.0, 0.0, 0.0]), [0.0, 0.0, 30.0]));
    }

    #[test]
    fn an_upright_placement_keeps_the_up_velocity_up_whatever_the_yaw() {
        for yaw in [0.0, 141.0, 270.0] {
            assert!(near(world([0.0, yaw, 0.0], [0.0, 0.0, 15.0]), [0.0, 0.0, 15.0]), "yaw {yaw}");
        }
    }

    #[test]
    fn the_forward_follows_the_yaw_and_the_roll_turns_the_up() {
        assert!(near(world([0.0, 90.0, 0.0], [10.0, 0.0, 0.0]), [0.0, 10.0, 0.0]));
        // Left of a placement facing +x is +y.
        assert!(near(world([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]), [0.0, 10.0, 0.0]));
        // Rolled a quarter turn, the up points to the side (the game's, not
        // an arbitrary perpendicular of the forward).
        let up = world([0.0, 0.0, 90.0], [0.0, 0.0, 10.0]);
        assert!(up[2].abs() < 1e-2 && up[1].abs() > 9.9, "{up:?}");
    }

    #[test]
    fn the_axis_is_the_engines_own() {
        for a in [[283.351, 73.0768, 167.323], [359.0, 141.0, -101.0], [0.0, 199.297, 0.0]] {
            let want = math_iw4::angles_to_axis(a);
            let got = axis_from_angles_deg(a);
            for r in 0..3 {
                assert!(near(got[r], want[r]), "{a:?} row {r}");
            }
        }
    }

    #[test]
    fn positive_gravity_pulls_down() {
        // (The runtime subtracts the sampled authored gravity from the
        // height: a positive one falls; smoke rises on its velocity.)
        let g = fx_iw4::elem_gravity_accel_z(0.0025);
        assert!((g - 2.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod hide_tests {
    use super::Hide;

    #[test]
    fn the_hide_list_names_types_materials_and_effects() {
        let h = Hide::parse(" Tail, mat:godray ,fx:snow/ , bogus");
        assert!(!h.is_empty());
        assert!(h.hides(2, "fx/a", "plain"));
        assert!(!h.hides(0, "fx/a", "plain"));
        assert!(h.hides(0, "fx/a", "GFX_Godray_beam_win"));
        assert!(h.hides(1, "fx/snow/flake", "plain"));
        assert!(Hide::parse("").is_empty());
        assert!(!Hide::parse("").hides(2, "x", "y"));
    }
}
