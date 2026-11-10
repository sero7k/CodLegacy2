//! bo2mp: Black Ops III's effects into the effect catalog (its maps'
//! placed effects: Nuk3town's data streams, sparkles, butterflies).
//!
//! `fastfile_t7`'s walk keeps an effect's header (0x90 bytes) and each
//! element's `FxElemDef` (0x260 bytes) with its sample arrays and the names
//! its visuals hold; the `assets` lane links the materials and passes them
//! here as a `T7FxView`. No public T7 layout: the fields below were read off
//! retail `mp_nuketown_x` (90 effects, 351 elements) against Black Ops II's
//! `FxElemDef`, whose order Black Ops III keeps with additions:
//!
//! ```text
//! 0x00 i32 flags (the family's bits: 0x40/0x80 run relative, 0x0100_0000
//!      local velocity, 0x0200_0000 world velocity, 0x1000_0000 non-uniform
//!      size); 0x04 i32 a second flag word
//! 0x08 spawn (looping: interval msec, count; one-shot: count base, amplitude)
//! 0x10, 0x14 (new)
//! 0x18 spawnRange (base, amplitude: the camera distances the element spawns
//!      at; Nuk3town's glass impact splits its elements at 750)
//! 0x20 the far fade (base, amplitude: gone past base + amplitude)
//! 0x28 the near fade (base, amplitude: gone nearer than base)
//! 0x30 f32 spawnFrustumCullRadius; 0x34 spawnDelayMsec; 0x3c lifeSpanMsec
//! 0x44 spawnOrigin (3 ranges); 0x5c spawnOffsetRadius; 0x64 spawnOffsetHeight
//! 0x6c (new); 0x70 spawnAngles (3 ranges); 0x88 angularVelocity (3 ranges)
//! 0xa0 initialRotation; 0xa8 rotationAxis; 0xac gravity; 0xb4 reflectionFactor
//! 0xbc atlas (behavior, index, fps, loop count, column bits, row bits, ...)
//! 0xc8 u8 elemType, visualCount, velIntervalCount, 0xcb, 0xcc,
//!      0xcd visStateIntervalCount
//! 0x1f0 u8 sort order
//! ```
//!
//! The near and far fades are the runtime's fade in and fade out ranges
//! (Black Ops II's names, the other way round: an element spawned to 1500
//! units fades out from 1200 to 1500, and big glows fade in past 100).
//!
//! Velocity samples (96 bytes) have IW4's layout and are kept. A visual
//! sample is 80 bytes, base then amplitude of 40: colour bytes red first
//! (IW4's are blue first), rotation delta and total, size[2], four words
//! not read, scale; the colour's amplitude is the far end of the random
//! blend, as IW4's.
//!
//! Element types: 0 billboard, 1 and 2 oriented, 3 tail, 4 line (a tail run
//! forward, as Black Ops II's), 5 trail, 6 cloud (drawn as sprites), 7
//! model, 8 light, 12 lens flare, 13 decal, 14 runner or sound. Lights,
//! flares and decals play as silent sound elements (the effect's other
//! elements keep their places and counts).

use super::t6::{T6_FX_ELEM_DRAW_WITH_VIEWMODEL, T6_FX_ELEM_LINE};
use super::*;

/// One Black Ops III visual with its name resolved by the lane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum T7FxVisual {
    #[default]
    None,
    /// A material linked into the material catalog under this name (in the
    /// map's namespace, Black Ops II's).
    Material(String),
    Model(String),
    Effect(String),
    Sound(String),
}

/// One Black Ops III element as the lane read it.
#[derive(Clone, Debug, Default)]
pub struct T7FxElemView {
    /// The 0x260-byte record.
    pub record: Vec<u8>,
    pub vel_samples: Vec<u8>,
    pub vis_samples: Vec<u8>,
    pub visuals: Vec<T7FxVisual>,
    pub effect_on_impact: Option<String>,
    pub effect_on_death: Option<String>,
    pub effect_emitted: Option<String>,
    /// A trail's record (0x30), vertices (20 bytes each) and indices.
    pub trail: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
    /// A light element's light description (0x240 bytes).
    pub light: Option<Vec<u8>>,
}

/// bo2mp: a Black Ops III light element as `fastfile_t7::scene_light`
/// takes it: its light description, its own visual samples (80 bytes
/// each) and how many intervals they span.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct T7FxLight {
    pub description: Vec<u8>,
    pub vis_samples: Vec<u8>,
    pub vis_intervals: u8,
}

/// One Black Ops III effect as the lane read it.
#[derive(Clone, Debug, Default)]
pub struct T7FxView {
    pub name: String,
    /// The 0x90-byte header.
    pub header: Vec<u8>,
    pub elems: Vec<T7FxElemView>,
}

/// Black Ops III's element types.
pub mod t7_elem_type {
    pub const BILLBOARD: u8 = 0;
    pub const ORIENTED: u8 = 1;
    pub const ROTATED: u8 = 2;
    pub const TAIL: u8 = 3;
    pub const LINE: u8 = 4;
    pub const TRAIL: u8 = 5;
    pub const CLOUD: u8 = 6;
    pub const MODEL: u8 = 7;
    pub const LIGHT: u8 = 8;
    pub const LENS_FLARE: u8 = 12;
    pub const DECAL: u8 = 13;
    pub const NAMED: u8 = 14;
}

const ELEM_TYPE: usize = 0xc8;
const VISUAL_COUNT: usize = 0xc9;
const VEL_INTERVALS: usize = 0xca;
const VIS_INTERVALS: usize = 0xcd;
const ATLAS: usize = 0xbc;
const SORT_ORDER: usize = 0x1f0;
const VEL_SAMPLE: usize = 96;
const T7_VIS_SAMPLE: usize = 80;
const IW4_VIS_SAMPLE: usize = 48;

/// Black Ops III element type -> the catalog's (IW4) type; `None` for one
/// drawn by nothing here (played as a silent sound element). A named
/// element is a runner or a sound by what its visuals name.
pub fn t7_elem_type(raw: u8, runs_effects: bool) -> Option<u8> {
    use t7_elem_type as t;
    Some(match raw {
        t::BILLBOARD | t::CLOUD => elem_type::BILLBOARD,
        t::ORIENTED | t::ROTATED => elem_type::ORIENTED,
        t::TAIL | t::LINE => elem_type::TAIL,
        t::TRAIL => elem_type::TRAIL,
        t::MODEL => elem_type::MODEL,
        t::NAMED if runs_effects => elem_type::RUNNER,
        t::NAMED => elem_type::SOUND,
        _ => return None,
    })
}

fn rd_i32(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4).and_then(|s| s.try_into().ok()).map_or(0, i32::from_le_bytes)
}

fn rd_f32(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4).and_then(|s| s.try_into().ok()).map_or(0.0, f32::from_le_bytes)
}

fn rd_i16(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 2).and_then(|s| s.try_into().ok()).map_or(0, |v| i32::from(i16::from_le_bytes(v)))
}

/// The element's flags in the runtime's terms: the family's bits, without
/// the two the runtime gives its own meaning (Black Ops II's first-person
/// and line bits), the line bit set for a line.
pub fn t7_elem_flags(raw_flags: i32, raw_type: u8) -> i32 {
    let mut flags = raw_flags & !(T6_FX_ELEM_DRAW_WITH_VIEWMODEL | T6_FX_ELEM_LINE);
    if raw_type == t7_elem_type::LINE {
        flags |= T6_FX_ELEM_LINE;
    }
    flags
}

/// The atlas as IW4's (behavior, index, fps, loop count, column bits, row
/// bits, entry count): the cells the bits make. A flipbook played over the
/// element's life (Nuk3town's voxel dissolve, 8 x 4) starts at its first
/// cell; any other picks its first at random.
pub fn t7_atlas(r: &[u8]) -> [u8; 8] {
    let b = |o: usize| r.get(ATLAS + o).copied().unwrap_or(0);
    let (col, row) = (b(4) & 7, b(5) & 7);
    let cells: u16 = 1 << (col + row);
    if cells <= 1 {
        return [0, 0, 0, 0, 0, 0, 1, 0];
    }
    const LIFE_SCALE: u8 = 4;
    let behavior = b(0);
    let (start, index) = if behavior & LIFE_SCALE != 0 { (0, 0) } else { (1, b(1)) };
    let [lo, hi] = cells.to_le_bytes();
    [start | (behavior & LIFE_SCALE), index, b(2), b(3).max(1), col, row, lo, hi]
}

fn t7_elem_view(r: &[u8], runs_effects: bool) -> (FxElemDefView, [u8; 8]) {
    let f = |off: usize| rd_f32(r, off);
    let i = |off: usize| rd_i32(r, off);
    let range = |off: usize| [f(off), f(off + 4)];
    let b = |off: usize| r.get(off).copied().unwrap_or(0);
    let raw_type = b(ELEM_TYPE);
    let mapped = t7_elem_type(raw_type, runs_effects);
    let view = FxElemDefView {
        flags: t7_elem_flags(i(0), raw_type),
        spawn_a: i(0x08),
        spawn_b: i(0x0c),
        spawn_range_base: f(0x18),
        spawn_range_amplitude: f(0x1c),
        fade_in_range: range(0x28),
        fade_out_range: range(0x20),
        spawn_frustum_cull_radius: f(0x30),
        spawn_delay_msec_base: i(0x34),
        spawn_delay_msec_amplitude: i(0x38),
        life_span_msec_base: i(0x3c),
        life_span_msec_amplitude: i(0x40),
        spawn_origin: core::array::from_fn(|a| range(0x44 + a * 8)),
        spawn_offset_radius_base: f(0x5c),
        spawn_offset_radius_amplitude: f(0x60),
        spawn_offset_height_base: f(0x64),
        spawn_offset_height_amplitude: f(0x68),
        spawn_angles: core::array::from_fn(|a| range(0x70 + a * 8)),
        angular_velocity: core::array::from_fn(|a| range(0x88 + a * 8)),
        initial_rotation: range(0xa0),
        gravity_base: f(0xac),
        gravity_amplitude: f(0xb0),
        reflection_factor: range(0xb4),
        coll_mins: [0.0; 3],
        coll_maxs: [0.0; 3],
        elem_type: mapped.unwrap_or(elem_type::SOUND),
        visual_count: if mapped.is_some() { b(VISUAL_COUNT) } else { 1 },
        vel_interval_count: b(VEL_INTERVALS),
        vis_state_interval_count: b(VIS_INTERVALS),
        // (Its emissive sprites are drawn unlit.)
        lighting_frac: 0,
        use_item_clip: 0,
        sort_order: b(SORT_ORDER),
        emit_dist: [0.0; 2],
        emit_dist_variance: [0.0; 2],
    };
    (view, t7_atlas(r))
}

/// Stops of brightness per unit of a visual sample's word 6 (see
/// `t7_vis_sample_to_iw4`).
const EMISSIVE_STOPS: f32 = 4.0;

/// The most the emissive light past white raises a sample's alpha.
const EMISSIVE_ALPHA_GAIN: f32 = 2.0;

/// One Black Ops III visual sample (80 bytes) as IW4's (48).
///
/// Word 6 of each half reads as the sprite's emissive intensity on a log
/// scale (0 on the big soft glows, which stay as their colour; 1 on the
/// data streams' voxels; rising over a light ray's life): the colour is
/// brightened by `2 ^ (EMISSIVE_STOPS * w)` and, past white, scaled back
/// keeping its hue (base and range by the same factor), as the effect pass
/// draws in display space; the factor goes into the alpha (at most `EMISSIVE_ALPHA_GAIN` times, at
/// most 1).
pub fn t7_vis_sample_to_iw4(s: &[u8]) -> [u8; IW4_VIS_SAMPLE] {
    let colour = |half: usize| -> [f32; 4] {
        let src = half * T7_VIS_SAMPLE / 2;
        let c = |k: usize| f32::from(s.get(src + k).copied().unwrap_or(0)) / 255.0;
        let w = rd_f32(s, src + 24);
        let gain = if w.is_finite() && (0.0..=2.0).contains(&w) { (EMISSIVE_STOPS * w).exp2() } else { 1.0 };
        [c(0) * gain, c(1) * gain, c(2) * gain, c(3)]
    };
    let (base, amp) = (colour(0), colour(1));
    let peak = (0..3).map(|k| base[k].max(amp[k])).fold(0.0f32, f32::max);
    let squeeze = if peak > 1.0 && peak.is_finite() { 1.0 / peak } else { 1.0 };
    // (Light past white loses its hue as the game's tone curve takes it:
    // the scaled-back colour is lifted toward white by `1 - peak^-1/4`,
    // so Nuk3town's orange voxels, 16 times over, glow gold-peach as in
    // the game, not rust brown.)
    let white = if squeeze < 1.0 { 1.0 - squeeze.powf(0.25) } else { 0.0 };
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut out = [0u8; IW4_VIS_SAMPLE];
    for (half, c) in [base, amp].into_iter().enumerate() {
        let src = half * T7_VIS_SAMPLE / 2;
        let o = half * IW4_VIS_SAMPLE / 2;
        let lift = |x: f32| x * squeeze * (1.0 - white) + white;
        out[o] = byte(lift(c[2]));
        out[o + 1] = byte(lift(c[1]));
        out[o + 2] = byte(lift(c[0]));
        // (The light past white goes into the alpha, which an added sprite
        // multiplies its colour by: Nuk3town's voxel streams, orange at
        // 16 times over an alpha of 0.06 to 0.13, drew as faint white
        // outlines once their colour was scaled back to white; their
        // bodies glow gold in the game.)
        out[o + 3] = byte(c[3] * (1.0 / squeeze).min(EMISSIVE_ALPHA_GAIN));
        for (k, at) in [4usize, 8, 12, 16, 36].into_iter().enumerate() {
            out[o + 4 + 4 * k..o + 8 + 4 * k].copy_from_slice(&rd_f32(s, src + at).to_le_bytes());
        }
    }
    out
}

fn t7_material_visual(name: &str) -> OwnedFxVisual {
    if name.is_empty() {
        return OwnedFxVisual::None;
    }
    OwnedFxVisual::Material {
        material: FxElemMaterial::Unresolved(AssetEdgeReason::CatalogMiss),
        hint: Some(name.to_owned()),
        material_namespace: crate::AssetNamespace::T6,
        authored: AuthoredRef { slot: true, alias: false },
    }
}

fn t7_visuals(el: &T7FxElemView, view: &FxElemDefView, supported: bool) -> Vec<OwnedFxVisual> {
    if !supported || el.visuals.is_empty() {
        return vec![OwnedFxVisual::None];
    }
    let out: Vec<OwnedFxVisual> = el
        .visuals
        .iter()
        .map(|v| match (v, view.elem_type) {
            (T7FxVisual::Material(name), _) => t7_material_visual(name),
            (T7FxVisual::Model(name), elem_type::MODEL) => model_visual(Some(name.clone())),
            (T7FxVisual::Effect(name), elem_type::RUNNER) => runner_visual(name.clone()),
            (T7FxVisual::Sound(name), elem_type::SOUND) => sound_visual(name.clone()),
            _ => OwnedFxVisual::None,
        })
        .collect();
    if out.is_empty() { vec![OwnedFxVisual::None] } else { out }
}

fn t7_trail((record, verts, inds): &(Vec<u8>, Vec<u8>, Vec<u8>)) -> OwnedFxTrailDef {
    OwnedFxTrailDef {
        scroll_time_msec: rd_i32(record, 0),
        repeat_dist: rd_i32(record, 4),
        inv_split_dist: rd_f32(record, 8),
        inv_split_arc_dist: rd_f32(record, 0xc),
        inv_split_time: rd_f32(record, 0x10),
        verts: verts
            .chunks_exact(20)
            .map(|v| FxTrailVertex {
                pos: [rd_f32(v, 0), rd_f32(v, 4)],
                normal: [rd_f32(v, 8), rd_f32(v, 12)],
                tex_coord: rd_f32(v, 16),
            })
            .collect(),
        inds: inds.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
    }
}

fn t7_elem(el: &T7FxElemView) -> OwnedFxElemDef {
    let raw_type = el.record.get(ELEM_TYPE).copied().unwrap_or(0);
    let runs_effects = el.visuals.iter().any(|v| matches!(v, T7FxVisual::Effect(_)));
    let supported = t7_elem_type(raw_type, runs_effects).is_some();
    let (mut view, atlas) = t7_elem_view(&el.record, runs_effects);
    // A light element with its light description runs as a light (its
    // scene light is the exe's, `fastfile_t7::scene_light`); without one
    // it stays silent.
    if raw_type == t7_elem_type::LIGHT && el.light.is_some() {
        view.elem_type = elem_type::OMNI_LIGHT;
    }
    let vel_count = usize::from(view.vel_interval_count) + 1;
    let vel_samples = el.vel_samples.get(..vel_count * VEL_SAMPLE).unwrap_or(&el.vel_samples).to_vec();
    let vis_samples: Vec<u8> = el
        .vis_samples
        .chunks_exact(T7_VIS_SAMPLE)
        .take(usize::from(view.vis_state_interval_count) + 1)
        .flat_map(t7_vis_sample_to_iw4)
        .collect();
    let vel_graph_local = parse_vel_graph_channel(&vel_samples, vel_count, false);
    let vel_graph_world = parse_vel_graph_channel(&vel_samples, vel_count, true);
    let visuals = t7_visuals(el, &view, supported);
    let child = |name: &Option<String>| capture_named_child(name.clone().unwrap_or_default());
    let (effect_on_impact, effect_on_impact_hint) = child(&el.effect_on_impact);
    let (effect_on_death, effect_on_death_hint) = child(&el.effect_on_death);
    let (effect_emitted, effect_emitted_hint) = child(&el.effect_emitted);
    let trail_def = (view.elem_type == elem_type::TRAIL).then(|| el.trail.as_ref().map(t7_trail)).flatten();
    OwnedFxElemDef {
        raw: leftover_pack_iw4_elem_raw(&view, atlas),
        view,
        vel_samples,
        vel_graph_local,
        vel_graph_world,
        vis_samples,
        visuals,
        effect_on_impact,
        effect_on_impact_hint,
        effect_on_death,
        effect_on_death_hint,
        effect_emitted,
        effect_emitted_hint,
        has_extended: trail_def.is_some(),
        trail_def,
        spark_fountain_def: None,
        cloud_density: None,
        h1: None,
        iw6: None,
    }
}

impl FxCatalog {
    /// bo2mp: one Black Ops III effect, under Black Ops II's namespace (a
    /// map's effects are the map's). False for an unnamed one or one whose
    /// element counts do not add up.
    pub fn capture_t7(&mut self, fx: &T7FxView) -> bool {
        self.capture_t7_in(crate::AssetNamespace::T6, fx)
    }

    /// bo2mp: as `capture_t7`, the effect kept under `ns` (Black Ops III's
    /// own namespace for a guest gun's effects, whose names meet no other
    /// game's).
    pub fn capture_t7_in(&mut self, ns: crate::AssetNamespace, fx: &T7FxView) -> bool {
        let name = fx.name.trim_start_matches(',');
        if name.is_empty() {
            return false;
        }
        let h = &fx.header;
        let view = FxEffectDefView {
            // (Black Ops III's effect flags are not IW4's: none kept.)
            flags: 0,
            msec_looping_life: rd_i32(h, 0x18),
            looping_count: rd_i16(h, 0x0c).max(0),
            one_shot_count: rd_i16(h, 0x0e).max(0),
            emission_count: rd_i16(h, 0x10).max(0),
        };
        let total = view.looping_count + view.one_shot_count + view.emission_count;
        if total as usize != fx.elems.len() {
            self.capture_gaps += 1;
            return false;
        }
        let elems: Vec<OwnedFxElemDef> = fx.elems.iter().map(t7_elem).collect();
        for (i, (el, def)) in fx.elems.iter().zip(&elems).enumerate() {
            if let (elem_type::OMNI_LIGHT, Some(d)) = (def.view.elem_type, &el.light) {
                self.t7_lights.insert(
                    (name.to_ascii_lowercase(), i as u8),
                    T7FxLight { description: d.clone(), vis_samples: el.vis_samples.clone(), vis_intervals: def.view.vis_state_interval_count },
                );
            }
        }
        self.last_captured = Some(name.to_owned());
        self.insert_owned(OwnedFxEffectDef {
            namespace: ns,
            name: name.to_owned(),
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

    #[test]
    fn colour_turns_blue_first_and_size_scale_carry() {
        let mut s = vec![0u8; 80];
        s[0..4].copy_from_slice(&[0xc2, 0x0f, 0x01, 0x40]);
        s[12..16].copy_from_slice(&181.0f32.to_le_bytes());
        s[16..20].copy_from_slice(&1267.5f32.to_le_bytes());
        s[36..40].copy_from_slice(&1.0f32.to_le_bytes());
        let o = t7_vis_sample_to_iw4(&s);
        assert_eq!(&o[0..4], &[0x01, 0x0f, 0xc2, 0x40]);
        // An emissive word of 1 brightens it 16 times: red is past white,
        // so the colour is scaled back to it keeping the hue, then lifted
        // toward white by 1 - (1 / peak)^(1/4).
        s[24..28].copy_from_slice(&1.0f32.to_le_bytes());
        let o = t7_vis_sample_to_iw4(&s);
        assert_eq!(o[2], 255);
        let white = 1.0 - (255.0f32 / (194.0 * 16.0)).powf(0.25);
        assert_eq!(o[1], ((15.0f32 / 194.0 * (1.0 - white) + white) * 255.0).round() as u8);
        assert_eq!(f32::from_le_bytes(o[12..16].try_into().unwrap()), 181.0);
        assert_eq!(f32::from_le_bytes(o[16..20].try_into().unwrap()), 1267.5);
        assert_eq!(f32::from_le_bytes(o[20..24].try_into().unwrap()), 1.0);
    }

    #[test]
    fn life_flipbook_starts_at_its_first_cell() {
        let mut r = vec![0u8; 0x260];
        r[ATLAS..ATLAS + 8].copy_from_slice(&[0x9f, 0x0f, 0, 1, 3, 2, 0x11, 0]);
        assert_eq!(t7_atlas(&r), [4, 0, 0, 1, 3, 2, 32, 0]);
        r[ATLAS..ATLAS + 8].copy_from_slice(&[0x9f, 0x0f, 0, 1, 0, 0, 0x11, 0]);
        assert_eq!(t7_atlas(&r)[6], 1);
    }

    #[test]
    fn named_elements_run_or_sound() {
        assert_eq!(t7_elem_type(14, true), Some(elem_type::RUNNER));
        assert_eq!(t7_elem_type(14, false), Some(elem_type::SOUND));
        assert_eq!(t7_elem_type(13, false), None);
        assert_eq!(t7_elem_type(4, false), Some(elem_type::TAIL));
    }
}
