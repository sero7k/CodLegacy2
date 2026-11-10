//! bo2mp: Infinite Warfare's particle systems into the effect catalog, for
//! its guest guns' own muzzle flashes, shell ejects and tracers.
//!
//! A particle system is not a classic effect: it is emitters, each with
//! modules (spawn, attributes, material, colour/size/velocity graphs over
//! the particle's life). The `assets` lane reads those
//! (`fastfile_iw7::walk::vfx`) and hands over a [`ParticleFxView`]: per
//! emitter the numbers the effect element needs, the graphs already sampled
//! at even steps (the element's intervals are even, `fx_iw4::visual`). This
//! builds the catalog's (IW4) element from it:
//!
//! - a visual sample is the colour (bytes, blue first) and size at a step,
//!   base then amplitude (the particle's random blend between the graph's
//!   two sets, a minimum and a maximum);
//! - velocity samples are IW4's layout (local velocity per millisecond, base
//!   and amplitude, then the integrated delta, which the runtime does not
//!   read), world velocity zero;
//! - spawn is a one-shot burst (count range) or a loop (interval, count);
//! - an emitter that runs other systems is a runner element.

use super::t6::T6_FX_ELEM_DRAW_WITH_VIEWMODEL;
use super::*;

/// What a particle emitter draws as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    /// A camera-facing sprite.
    Billboard,
    /// A sprite with its own plane.
    Oriented,
    /// A sprite stretched along its velocity (a tracer, a spark).
    Tail,
    /// A model (a shell casing).
    Model,
    /// Runs other effects.
    Runner,
}

/// One step of a particle's life: the graph's minimum set and maximum set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleStep {
    /// Linear colour and alpha, 0..1 (HDR already folded in).
    pub colour: [[f32; 4]; 2],
    pub size: [[f32; 2]; 2],
    /// Velocity per second (effect-local) at this step, minimum and maximum.
    pub velocity: [[f32; 3]; 2],
}

/// One emitter, ready to become an element.
#[derive(Clone, Debug)]
pub struct ParticleElemView {
    pub kind: ParticleKind,
    /// `Some((interval msec, count))` loops; `None` is a one-shot burst of
    /// `burst` (minimum, maximum) particles.
    pub looping: Option<(i32, i32)>,
    pub burst: [i32; 2],
    /// Seconds, as authored.
    pub delay: [f32; 2],
    pub life: [f32; 2],
    pub steps: Vec<ParticleStep>,
    /// Whether the element has a velocity graph at all.
    pub has_velocity: bool,
    pub gravity: [f32; 2],
    /// Spawn box: per axis (minimum, maximum).
    pub origin: [[f32; 2]; 3],
    pub offset_radius: [f32; 2],
    /// Degrees.
    pub initial_rotation: [f32; 2],
    /// The sprite materials (catalog names), models, and the effects run.
    pub materials: Vec<String>,
    pub models: Vec<String>,
    pub effects: Vec<String>,
    /// The flipbook: (columns, rows, frames per second, play over life).
    pub atlas: Option<(u32, u32, u32, bool)>,
    /// Drawn with the first-person gun.
    pub view_model: bool,
    pub sort_order: u8,
}

/// One particle system.
#[derive(Clone, Debug, Default)]
pub struct ParticleFxView {
    pub name: String,
    pub elems: Vec<ParticleElemView>,
    /// How long the looping elements run, milliseconds (0: from the
    /// elements' intervals).
    pub looping_life_ms: i32,
}

const FX_RUN_RELATIVE_TO_EFFECT: i32 = 0x80;
const FX_VEL_LOCAL: i32 = 0x0100_0000;

fn ms(seconds: f32) -> i32 {
    (seconds * 1000.0).round().clamp(0.0, 1.0e7) as i32
}

fn byte(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A step's visual sample as IW4's (48 bytes): the colour is the minimum
/// set with the span to the maximum set as its amplitude (a byte cannot go
/// below zero), the size is the minimum set and the signed span.
fn vis_sample(step: &ParticleStep) -> [u8; 48] {
    let mut out = [0u8; 48];
    let (lo, hi) = (step.colour[0], step.colour[1]);
    let base: [f32; 4] = core::array::from_fn(|k| lo[k].min(hi[k]));
    let amp: [f32; 4] = core::array::from_fn(|k| lo[k].max(hi[k]) - base[k]);
    for (half, c) in [base, amp].into_iter().enumerate() {
        let o = half * 24;
        // (Blue, green, red, alpha.)
        out[o] = byte(c[2]);
        out[o + 1] = byte(c[1]);
        out[o + 2] = byte(c[0]);
        out[o + 3] = byte(c[3]);
        let size = if half == 0 {
            step.size[0]
        } else {
            [
                step.size[1][0] - step.size[0][0],
                step.size[1][1] - step.size[0][1],
            ]
        };
        // rotation delta, rotation total, size x2, scale
        for (k, v) in [0.0f32, 0.0, size[0], size[1], 1.0].into_iter().enumerate() {
            out[o + 4 + 4 * k..o + 8 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// A step's velocity sample as IW4's (96 bytes): the local frame's velocity
/// (units per millisecond), base then amplitude, and its integrated delta
/// (not read), then the world frame's, zero.
fn vel_sample(step: &ParticleStep) -> [u8; 96] {
    let mut out = [0u8; 96];
    let (lo, hi) = (step.velocity[0], step.velocity[1]);
    for k in 0..3 {
        let base = lo[k] / 1000.0;
        let amp = (hi[k] - lo[k]) / 1000.0;
        out[4 * k..4 * k + 4].copy_from_slice(&base.to_le_bytes());
        out[12 + 4 * k..16 + 4 * k].copy_from_slice(&amp.to_le_bytes());
    }
    out
}

/// A power of two's exponent (`None` for any other number).
fn log2_exact(n: u32) -> Option<u8> {
    (n.is_power_of_two()).then(|| n.trailing_zeros() as u8)
}

fn particle_atlas(a: Option<(u32, u32, u32, bool)>) -> [u8; 8] {
    let Some((cols, rows, fps, over_life)) = a else {
        return [0, 0, 0, 0, 0, 0, 1, 0];
    };
    let (Some(cb), Some(rb)) = (log2_exact(cols), log2_exact(rows)) else {
        return [0, 0, 0, 0, 0, 0, 1, 0];
    };
    let cells = (cols * rows).min(0x7fff) as u16;
    if cells <= 1 {
        return [0, 0, 0, 0, 0, 0, 1, 0];
    }
    // (Bit 0: a random first cell; 4: the flipbook plays over the
    // particle's life; 8: stops on the last cell.)
    let behavior = 1 | if over_life { 4 } else { 0 };
    let [lo, hi] = cells.to_le_bytes();
    [behavior, 0, fps.min(255) as u8, 1, cb, rb, lo, hi]
}

fn particle_elem(el: &ParticleElemView) -> OwnedFxElemDef {
    let steps = el.steps.len().max(2);
    let mut sampled = el.steps.clone();
    sampled.resize(steps, el.steps.last().copied().unwrap_or_default());
    let mut flags = FX_RUN_RELATIVE_TO_EFFECT;
    if el.has_velocity {
        flags |= FX_VEL_LOCAL;
    }
    if el.view_model {
        flags |= T6_FX_ELEM_DRAW_WITH_VIEWMODEL;
    }
    let (elem_type, visuals) = match el.kind {
        ParticleKind::Billboard => (elem_type::BILLBOARD, materials_of(el)),
        ParticleKind::Oriented => (elem_type::ORIENTED, materials_of(el)),
        ParticleKind::Tail => (elem_type::TAIL, materials_of(el)),
        ParticleKind::Model => (
            elem_type::MODEL,
            el.models
                .iter()
                .cloned()
                .map(|m| model_visual(Some(m)))
                .collect(),
        ),
        ParticleKind::Runner => (
            elem_type::RUNNER,
            el.effects.iter().cloned().map(runner_visual).collect(),
        ),
    };
    let visuals = if visuals.is_empty() {
        vec![OwnedFxVisual::None]
    } else {
        visuals
    };
    let (spawn_a, spawn_b) = match el.looping {
        Some((interval, count)) => (interval, count),
        None => (el.burst[0], (el.burst[1] - el.burst[0]).max(0)),
    };
    let life = [ms(el.life[0]), ms((el.life[1] - el.life[0]).max(0.0))];
    let delay = [ms(el.delay[0]), ms((el.delay[1] - el.delay[0]).max(0.0))];
    let range = |r: [f32; 2]| [r[0], r[1] - r[0]];
    let view = FxElemDefView {
        flags,
        spawn_a,
        spawn_b,
        spawn_range_base: 0.0,
        spawn_range_amplitude: 0.0,
        fade_in_range: [0.0; 2],
        fade_out_range: [0.0; 2],
        spawn_frustum_cull_radius: 0.0,
        spawn_delay_msec_base: delay[0],
        spawn_delay_msec_amplitude: delay[1],
        life_span_msec_base: life[0],
        life_span_msec_amplitude: life[1],
        spawn_origin: core::array::from_fn(|a| range(el.origin[a])),
        spawn_offset_radius_base: el.offset_radius[0],
        spawn_offset_radius_amplitude: el.offset_radius[1] - el.offset_radius[0],
        spawn_offset_height_base: 0.0,
        spawn_offset_height_amplitude: 0.0,
        spawn_angles: [[0.0; 2]; 3],
        angular_velocity: [[0.0; 2]; 3],
        initial_rotation: range(el.initial_rotation),
        gravity_base: el.gravity[0],
        gravity_amplitude: el.gravity[1] - el.gravity[0],
        reflection_factor: [0.0; 2],
        coll_mins: [0.0; 3],
        coll_maxs: [0.0; 3],
        elem_type,
        visual_count: visuals.len().min(255) as u8,
        vel_interval_count: (steps - 1).min(255) as u8,
        vis_state_interval_count: (steps - 1).min(255) as u8,
        lighting_frac: 0,
        use_item_clip: 0,
        sort_order: el.sort_order,
        emit_dist: [0.0; 2],
        emit_dist_variance: [0.0; 2],
    };
    let vel_samples: Vec<u8> = sampled.iter().flat_map(vel_sample).collect();
    let vis_samples: Vec<u8> = sampled.iter().flat_map(vis_sample).collect();
    let vel_count = steps;
    let vel_graph_local = parse_vel_graph_channel(&vel_samples, vel_count, false);
    let vel_graph_world = parse_vel_graph_channel(&vel_samples, vel_count, true);
    let atlas = particle_atlas(el.atlas);
    OwnedFxElemDef {
        raw: leftover_pack_iw4_elem_raw(&view, atlas),
        view,
        vel_samples,
        vel_graph_local,
        vel_graph_world,
        vis_samples,
        visuals,
        effect_on_impact: FxChildEdge::Absent,
        effect_on_impact_hint: None,
        effect_on_death: FxChildEdge::Absent,
        effect_on_death_hint: None,
        effect_emitted: FxChildEdge::Absent,
        effect_emitted_hint: None,
        has_extended: false,
        trail_def: None,
        spark_fountain_def: None,
        cloud_density: None,
        h1: None,
        iw6: None,
    }
}

fn materials_of(el: &ParticleElemView) -> Vec<OwnedFxVisual> {
    el.materials
        .iter()
        .map(|m| OwnedFxVisual::Material {
            material: FxElemMaterial::Unresolved(AssetEdgeReason::CatalogMiss),
            hint: Some(m.clone()),
            material_namespace: crate::AssetNamespace::T6,
            authored: AuthoredRef {
                slot: true,
                alias: false,
            },
        })
        .collect()
}

impl FxCatalog {
    /// bo2mp: one Infinite Warfare particle system under `ns`. False for an
    /// unnamed one or one with no element.
    pub fn capture_particles_in(&mut self, ns: crate::AssetNamespace, fx: &ParticleFxView) -> bool {
        if fx.name.is_empty() || fx.elems.is_empty() {
            return false;
        }
        let looping: Vec<&ParticleElemView> =
            fx.elems.iter().filter(|e| e.looping.is_some()).collect();
        let one_shot: Vec<&ParticleElemView> =
            fx.elems.iter().filter(|e| e.looping.is_none()).collect();
        let msec_looping_life = if fx.looping_life_ms > 0 {
            fx.looping_life_ms
        } else {
            looping
                .iter()
                .filter_map(|e| {
                    e.looping
                        .map(|(interval, count)| interval.saturating_mul(count))
                })
                .max()
                .unwrap_or(0)
        };
        let view = FxEffectDefView {
            flags: 0,
            msec_looping_life,
            looping_count: looping.len() as i32,
            one_shot_count: one_shot.len() as i32,
            emission_count: 0,
        };
        let elems: Vec<OwnedFxElemDef> = looping
            .into_iter()
            .chain(one_shot)
            .map(particle_elem)
            .collect();
        self.last_captured = Some(fx.name.clone());
        self.insert_owned(OwnedFxEffectDef {
            namespace: ns,
            name: fx.name.clone(),
            view,
            header_raw: leftover_pack_iw4_effect_header(&view),
            elems,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flash() -> ParticleFxView {
        let step = |a: f32| ParticleStep {
            colour: [[1.0, 0.5, 0.25, a], [1.0, 0.75, 0.5, a]],
            size: [[10.0, 12.0], [20.0, 12.0]],
            velocity: [[100.0, 0.0, 0.0], [200.0, 0.0, 0.0]],
        };
        ParticleFxView {
            name: "vfx/test/flash".into(),
            elems: vec![ParticleElemView {
                kind: ParticleKind::Billboard,
                looping: None,
                burst: [1, 1],
                delay: [0.0, 0.0],
                life: [0.1, 0.2],
                steps: vec![step(0.0), step(1.0), step(0.0)],
                has_velocity: true,
                gravity: [0.0; 2],
                origin: [[0.0; 2]; 3],
                offset_radius: [0.0; 2],
                initial_rotation: [0.0; 2],
                materials: vec!["iw7fx:test".into()],
                models: vec![],
                effects: vec![],
                atlas: Some((2, 2, 30, false)),
                view_model: true,
                sort_order: 0,
            }],
            looping_life_ms: 0,
        }
    }

    #[test]
    fn a_flash_becomes_one_billboard_with_even_samples() {
        let mut fx = FxCatalog::default();
        assert!(fx.capture_particles_in(crate::AssetNamespace::Iw7, &flash()));
        let def = fx
            .get_in(crate::AssetNamespace::Iw7, "vfx/test/flash")
            .expect("captured");
        assert_eq!((def.view.one_shot_count, def.view.looping_count), (1, 0));
        let el = &def.elems[0];
        assert_eq!(el.view.elem_type, elem_type::BILLBOARD);
        assert_eq!(el.view.life_span_msec_base, 100);
        assert_eq!(el.view.life_span_msec_amplitude, 100);
        assert_eq!(el.view.vis_state_interval_count, 2);
        assert_eq!(el.vis_samples.len(), 3 * 48);
        assert_eq!(el.vel_samples.len(), 3 * 96);
        assert!(el.view.flags & T6_FX_ELEM_DRAW_WITH_VIEWMODEL != 0);
        // Base colour is blue first: red 1.0 in the third byte, alpha of the
        // second step is 255.
        let s1 = &el.vis_samples[48..96];
        assert_eq!((s1[0], s1[1], s1[2], s1[3]), (64, 128, 255, 255));
        // Size amplitude is the signed span to the maximum set.
        let amp = f32::from_le_bytes(s1[24 + 12..24 + 16].try_into().unwrap());
        assert_eq!(amp, 10.0);
        assert_eq!(el.vel_graph_local.len(), 3);
        assert!((el.vel_graph_local[0].base[0] - 0.1).abs() < 1e-6);
        assert_eq!(el.raw[0xa8 + 6], 4);
    }

    #[test]
    fn looping_and_runner_elements() {
        let mut fx = flash();
        let mut run = fx.elems[0].clone();
        run.kind = ParticleKind::Runner;
        run.effects = vec!["vfx/test/child".into()];
        run.materials.clear();
        let mut looped = fx.elems[0].clone();
        looped.looping = Some((50, 4));
        fx.elems = vec![run, looped];
        let mut cat = FxCatalog::default();
        assert!(cat.capture_particles_in(crate::AssetNamespace::Iw7, &fx));
        let def = cat
            .get_in(crate::AssetNamespace::Iw7, "vfx/test/flash")
            .expect("captured");
        assert_eq!(
            (
                def.view.looping_count,
                def.view.one_shot_count,
                def.view.msec_looping_life
            ),
            (1, 1, 200)
        );
        // (Looping elements come first.)
        assert_eq!(def.elems[0].view.spawn_a, 50);
        assert_eq!(def.elems[1].view.elem_type, elem_type::RUNNER);
        assert!(matches!(
            def.elems[1].visuals[0],
            OwnedFxVisual::Runner { .. }
        ));
    }
}
