//! bo2mp: Ghosts' effect lights (the omni and spot light elements of its
//! effects, types 12 and 13) handed to the native Ghosts renderer
//! (`render_iw6::fx_light`), which lights the world with the nearest few.
//!
//! The elements arrive as the draw dispatch's `FxElemLightInstance`s (the
//! exe's omni handler: origin, radius = the sample's size, colour times
//! scale, culled by its sphere; `fx_iw4::iw6::light_state`); this module
//! picks the most important ones under the graphics preset's cap and
//! copies them into the fixed list `asset_world::set_iw6_fx_lights_with`
//! holds, so nothing is allocated per frame. `BO2MP_IW6_FX_LIGHTS=0` turns
//! the lights off: nothing is handed over and the spot element is dropped
//! again (the omni element still plays as a light for the Black Ops II
//! renderer, as before).

use asset_world::{IW6_FX_LIGHTS_MAX, Iw6FxLight, Iw6FxLights};
use fx::FxElemLightInstance;

/// `BO2MP_IW6_FX_LIGHTS` (on): Ghosts' effect lights light the native
/// frame (and the spot light element, type 13, plays).
pub fn on() -> bool {
    // (Read once: asked for every frame.)
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| asset_audio::iw6_rules::switch("FX_LIGHTS", true))
}

/// The spot cone's half angle for an effect spot light of this radius:
/// Black Ops II's default cone (`R_SPOT_LIGHT_START/END_RADIUS`) scaled to
/// the radius. INFERRED: Ghosts' spot handler's cone was not decoded.
pub fn spot_cos_cone(radius: f32) -> f32 {
    let d = lighting_iw4::SpotLightConeDvars::register_defaults();
    let end = lighting_iw4::spot_light_clamp_end(d.start_radius, d.end_radius, radius.max(1.0));
    ((end - d.start_radius) / radius.max(1.0)).atan().cos()
}

/// How much a light matters at `eye`: the smaller the squared distance in
/// radii, the more (as Black Ops II orders its effect lights).
fn score(eye: [f32; 3], l: &FxElemLightInstance) -> f32 {
    let d: f32 = (0..3).map(|k| (l.origin[k] - eye[k]).powi(2)).sum();
    d / (l.radius * l.radius).max(1e-6)
}

fn to_light(l: &FxElemLightInstance) -> Iw6FxLight {
    let [b, g, r] = l.color_bgr;
    Iw6FxLight {
        origin: l.origin,
        radius: l.radius,
        color: [r, g, b],
        dir: l.axis[0],
        cos_cone: if l.is_spot { spot_cos_cone(l.radius) } else { 1.0 },
    }
}

/// The `cap` most important of `omni` and `spot` (nearest in radii first;
/// lights with no radius or no colour are left out) into `out`, in order.
/// No allocation: an insertion into the fixed list.
pub fn select(eye: [f32; 3], omni: &[FxElemLightInstance], spot: &[FxElemLightInstance], cap: usize, out: &mut Iw6FxLights) {
    out.len = 0;
    let cap = cap.min(IW6_FX_LIGHTS_MAX);
    if cap == 0 {
        return;
    }
    let mut scores = [0f32; IW6_FX_LIGHTS_MAX];
    for l in omni.iter().chain(spot) {
        if l.radius <= 0.0 || !l.color_bgr.iter().any(|c| *c > 0.0) {
            continue;
        }
        let s = score(eye, l);
        if !s.is_finite() {
            continue;
        }
        if out.len == cap && s >= scores[cap - 1] {
            continue;
        }
        let mut at = out.len.min(cap - 1);
        if out.len < cap {
            out.len += 1;
        }
        while at > 0 && scores[at - 1] > s {
            scores[at] = scores[at - 1];
            out.lights[at] = out.lights[at - 1];
            at -= 1;
        }
        scores[at] = s;
        out.lights[at] = to_light(l);
    }
}

/// Publishes the frame's effect lights for the native renderer: the
/// `cap` nearest. Off a native Ghosts map (or with the switch off, or the
/// preset's cap 0) it publishes none.
pub fn publish(eye: [f32; 3], omni: &[FxElemLightInstance], spot: &[FxElemLightInstance]) {
    let native = on() && asset_world::iw6_native_zones().is_some();
    let cap = if native { usize::from(frame::settings::native_quality::current().fx_lights) } else { 0 };
    asset_world::set_iw6_fx_lights_with(|out| select(eye, omni, spot, cap, out));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(x: f32, radius: f32, spot: bool) -> FxElemLightInstance {
        FxElemLightInstance {
            def_name: String::new(),
            def_index: 0,
            is_spot: spot,
            origin: [x, 0.0, 0.0],
            radius,
            color_bgr: [0.1, 0.2, 0.4],
            axis: [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        }
    }

    #[test]
    fn the_nearest_in_radii_win_in_order() {
        let omni = [light(1000.0, 100.0, false), light(100.0, 100.0, false), light(500.0, 1000.0, false)];
        let mut out = Iw6FxLights::default();
        select([0.0; 3], &omni, &[], 2, &mut out);
        assert_eq!(out.len, 2);
        // (d/r)^2: 1, 0.25, 100.
        assert_eq!(out.lights[0].origin[0], 500.0);
        assert_eq!(out.lights[1].origin[0], 100.0);
    }

    #[test]
    fn cap_zero_and_dead_lights_give_none() {
        let mut out = Iw6FxLights::default();
        select([0.0; 3], &[light(1.0, 10.0, false)], &[], 0, &mut out);
        assert_eq!(out.len, 0);
        let mut dark = light(1.0, 10.0, false);
        dark.color_bgr = [0.0; 3];
        select([0.0; 3], &[dark, light(1.0, 0.0, false)], &[], 4, &mut out);
        assert_eq!(out.len, 0);
    }

    #[test]
    fn colour_is_red_first_and_a_spot_has_a_cone() {
        let mut out = Iw6FxLights::default();
        select([0.0; 3], &[light(1.0, 200.0, false)], &[light(2.0, 200.0, true)], 4, &mut out);
        assert_eq!(out.len, 2);
        assert_eq!(out.lights[0].color, [0.4, 0.2, 0.1]);
        assert_eq!(out.lights[0].cos_cone, 1.0);
        let spot = out.lights.iter().find(|l| l.cos_cone < 1.0).unwrap();
        assert!(spot.cos_cone > 0.0 && spot.dir == [0.0, 0.0, -1.0]);
    }

    #[test]
    fn more_lights_than_the_list_hold_keep_the_best() {
        let omni: Vec<_> = (0..40).map(|k| light(10.0 * (40 - k) as f32, 100.0, false)).collect();
        let mut out = Iw6FxLights::default();
        select([0.0; 3], &omni, &[], 99, &mut out);
        assert_eq!(out.len, IW6_FX_LIGHTS_MAX);
        assert_eq!(out.lights[0].origin[0], 10.0);
        assert!(out.as_slice().windows(2).all(|w| w[0].origin[0] <= w[1].origin[0]));
    }
}
