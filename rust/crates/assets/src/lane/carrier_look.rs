//! bo2mp: Carrier (`mp_carrier`) in the owner's target look
//! (`docs/look_refs/carrier_target`): a late dusk, the sun low and warm, long
//! shadows, a stormy sky, a wet deck with puddles, a dark sea, fog thickening
//! to the horizon, a cool shadow tint. Black Ops II's clear daytime lighting
//! on Carrier is replaced, with no switch back (`BO2MP_CARRIER_LOOK=0` is a
//! test aid only).
//!
//! It rides Black Ops III's look on a Black Ops II map (`super::bo3_look`,
//! forced on for Carrier) and changes what that look is given:
//! * the sun (`sun`): the map's own yaw, lowered; its colour warmed;
//! * the fog (`fog`, the two fog rows, the fog banks): cool grey-blue away
//!   from the sun, warm toward it, denser toward the horizon;
//! * a look row (light row 251, vec4s 1004..1007: `write_look_row`) that
//!   `geometry_diagnostic.wgsl` reads (`look_on`, `look_row`): wetness and
//!   puddles, the ambient light's cool tint, the sky's storm grade and
//!   clouds, the warm glow round the sun;
//! * the sea's colour (`sea_tint`, the open-sea water material).
//!
//! Every number is tunable (`BO2MP_CARRIER_*`); the README lists them.

use std::sync::atomic::{AtomicBool, Ordering};

use asset_t6::WorldFogRef;
use asset_world::WorldDraw;

/// The light-table row of the look (four vec4s, 1004..1007).
pub(super) const LOOK_ROW: usize = 251;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the map is drawn in this look. (`BO2MP_CARRIER_LOOK=0` turns it off.)
/// Off by default (owner, 2026-10-27: Carrier is drawn as Black Ops II has
/// it again, to be upscaled with the other maps later); `BO2MP_CARRIER_LOOK=1`
/// turns the dusk look back on for testing.
pub(super) fn on(map: &str) -> bool {
    map.eq_ignore_ascii_case("mp_carrier")
        && std::env::var("BO2MP_CARRIER_LOOK").is_ok_and(|v| v.trim() == "1")
}

/// Records which map is loading (the sea's material is built before the
/// world is drawn).
pub(super) fn activate(map: &str) {
    ACTIVE.store(on(map), Ordering::Relaxed);
}

fn env_f32(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

/// `BO2MP_CARRIER_*="r g b"` (spaces or commas), else the default.
fn env_rgb(name: &str, default: [f32; 3]) -> [f32; 3] {
    let Ok(text) = codlegacy_env::var(name) else {
        return default;
    };
    let v: Vec<f32> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if v.len() == 3 && v.iter().all(|x| x.is_finite() && *x >= 0.0) {
        [v[0], v[1], v[2]]
    } else {
        default
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// `colour` with the luminance of `like`.
fn with_luminance(colour: [f32; 3], like: f32) -> [f32; 3] {
    let l = lum(colour).max(1e-6);
    colour.map(|c| c / l * like)
}

/// The exposure Carrier's dusk is drawn at (`BO2MP_CARRIER_EXPOSURE`).
pub(super) fn exposure() -> f32 {
    env_f32("BO2MP_CARRIER_EXPOSURE", 1.3).max(0.01)
}

/// The dusk sun from the map's own: (direction toward it, colour). The yaw is
/// kept, the elevation lowered (`BO2MP_CARRIER_SUN_ELEV`, degrees, default 9);
/// the colour keeps the original luminance, warmed (1, .56, .30), times
/// `BO2MP_CARRIER_SUN`. A low sun lights an up-facing surface by sin(elevation),
/// so the deck would go dark: the sun is raised by the square root of the lost
/// N.L (at most 2.2x, none if the original was lower), which keeps the deck's
/// light in step while the shadows stay long.
pub(super) fn sun(
    dir: [f32; 3],
    colour: [f32; 3],
    report: &mut Vec<String>,
) -> ([f32; 3], [f32; 3]) {
    let elev = env_f32("BO2MP_CARRIER_SUN_ELEV", 9.0)
        .clamp(1.0, 60.0)
        .to_radians();
    let yaw = dir[1].atan2(dir[0]);
    let new = [elev.cos() * yaw.cos(), elev.cos() * yaw.sin(), elev.sin()];
    let was = dir[2].clamp(0.05, 1.0);
    let lift = (was / elev.sin()).sqrt().clamp(1.0, 2.2);
    let tint = env_rgb("BO2MP_CARRIER_SUN_TINT", [1.0, 0.56, 0.30]);
    let c = with_luminance(tint, lum(colour) * env_f32("BO2MP_CARRIER_SUN", 1.0) * lift);
    report.push(format!("carrier look: sun {dir:.3?} -> {new:.3?} (elevation {:.1} deg), colour {colour:.2?} -> {c:.2?} (low-sun lift x{lift:.2})", elev.to_degrees()));
    (new, c)
}

/// The map's fog in the dusk's colours: cool grey-blue (0.42, 0.47, 0.55)
/// away from the sun and warm orange (1, 0.62, 0.35) toward it, each at the
/// original colour's luminance; the sun fog's direction the new sun, its cone
/// wide; the half distance 0.6x (`BO2MP_CARRIER_FOG`: the factor), so it
/// thickens toward the horizon.
pub(super) fn fog(f: WorldFogRef, sun_dir: [f32; 3]) -> WorldFogRef {
    let mut o = f;
    o.fog_color = with_luminance(
        env_rgb("BO2MP_CARRIER_FOG_COLOUR", [0.42, 0.47, 0.55]),
        lum(f.fog_color),
    );
    let warm_lum = lum(f.sun_fog_color).max(lum(f.fog_color));
    o.sun_fog_color = with_luminance(
        env_rgb("BO2MP_CARRIER_FOG_SUN", [1.0, 0.62, 0.35]),
        warm_lum,
    );
    // (The game's angles: pitch down is positive.)
    o.sun_fog_pitch = (-sun_dir[2]).clamp(-1.0, 1.0).asin().to_degrees();
    o.sun_fog_yaw = sun_dir[1].atan2(sun_dir[0]).to_degrees();
    o.sun_fog_inner = 15.0;
    o.sun_fog_outer = 80.0;
    o.sun_fog_opacity = f.sun_fog_opacity.max(f.fog_opacity);
    o.half_dist = (f.half_dist * env_f32("BO2MP_CARRIER_FOG", 0.6)).max(1.0);
    o
}

/// The sky's overhead colour (`BO2MP_CARRIER_SKY_COLOUR`), which the
/// real-time ambient (`super::rt_ambient`) takes as its zenith.
pub(super) fn sky_colour() -> [f32; 3] {
    env_rgb("BO2MP_CARRIER_SKY_COLOUR", [0.50, 0.58, 0.70])
}

/// The sea's colour factor (the open-sea water material; the sun glitter
/// follows the sun by itself): `BO2MP_CARRIER_SEA` (default 0.55) times a cool
/// tint. None off Carrier or at 1.
pub(super) fn sea_tint() -> Option<[f32; 3]> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    let s = env_f32("BO2MP_CARRIER_SEA", 0.55);
    (s != 1.0).then(|| [0.85 * s, 0.95 * s, 1.10 * s])
}

/// Writes the look row (`LOOK_ROW`): v0 (wetness, puddle scale, puddle
/// threshold, 1 = on), v1 (ambient tint rgb, ambient scale), v2 (sky overhead
/// grade rgb, amount), v3 (sun glow colour rgb, glow sharpness). Skipped, and
/// said so, if a primary light holds the row.
pub(super) fn write_look_row(draw: &mut WorldDraw, sun_colour: [f32; 3], report: &mut Vec<String>) {
    if draw.t6_lights.len() <= LOOK_ROW {
        draw.t6_lights.resize(LOOK_ROW + 1, [0.0; 16]);
    }
    if draw.t6_lights[LOOK_ROW].iter().any(|v| *v != 0.0) {
        report.push(format!("carrier look: row {LOOK_ROW} holds a primary light: wetness, sky grade and glow skipped"));
        return;
    }
    let wet = env_f32("BO2MP_CARRIER_WET", 1.0);
    let puddle_scale = env_f32("BO2MP_CARRIER_PUDDLE_SCALE", 0.008);
    let puddle_threshold = env_f32("BO2MP_CARRIER_PUDDLE_COVER", 0.55);
    let amb = env_f32("BO2MP_CARRIER_AMBIENT", 0.6);
    let amb_tint = env_rgb("BO2MP_CARRIER_AMBIENT_TINT", [0.80, 0.88, 1.10]);
    let sky = env_f32("BO2MP_CARRIER_SKY", 0.7);
    let sky_grade = env_rgb("BO2MP_CARRIER_SKY_COLOUR", [0.50, 0.58, 0.70]);
    // The glow is in the sky's own units, which follow the sun's strength.
    let glow = env_f32("BO2MP_CARRIER_GLOW", 1.0) * 0.35 * lum(sun_colour);
    let glow_rgb = [1.0, 0.55, 0.25].map(|c| c * glow);
    let sharp = env_f32("BO2MP_CARRIER_GLOW_POW", 6.0).max(1.0);
    draw.t6_lights[LOOK_ROW] = [
        wet,
        puddle_scale,
        puddle_threshold,
        1.0,
        amb_tint[0],
        amb_tint[1],
        amb_tint[2],
        amb,
        sky_grade[0],
        sky_grade[1],
        sky_grade[2],
        sky,
        glow_rgb[0],
        glow_rgb[1],
        glow_rgb[2],
        sharp,
    ];
    report.push(format!(
        "carrier look: row {LOOK_ROW} wet {wet} puddles scale {puddle_scale} threshold {puddle_threshold}, ambient x{amb} tint {amb_tint:.2?}, sky {sky} {sky_grade:.2?}, glow {glow_rgb:.2?} ^{sharp}"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_look_is_off_by_default() {
        // (Off unless BO2MP_CARRIER_LOOK=1; never on another map.)
        if std::env::var("BO2MP_CARRIER_LOOK").is_err() {
            assert!(!on("mp_carrier"));
        }
        assert!(!on("mp_nuketown_2020"));
    }

    #[test]
    fn the_sun_keeps_its_yaw_and_is_lowered() {
        let mut r = Vec::new();
        let d = [0.5f32, 0.5, 0.7071];
        let (n, c) = sun(d, [3.0, 3.0, 3.0], &mut r);
        assert!((n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-4);
        assert!(n[2] < d[2] && n[2] > 0.0);
        assert!((n[1].atan2(n[0]) - d[1].atan2(d[0])).abs() < 1e-4);
        // Warm: red over blue.
        assert!(c[0] > c[2]);
    }

    #[test]
    fn the_fog_keeps_its_luminance_and_turns_cool() {
        let f = WorldFogRef {
            half_dist: 4000.0,
            fog_opacity: 1.0,
            fog_color: [0.81, 0.96, 0.96],
            sun_fog_color: [1.0, 1.0, 1.0],
            sun_fog_opacity: 0.5,
            half_height: 1.0,
            ..Default::default()
        };
        let o = fog(f, [0.0, 1.0, 0.1]);
        assert!((lum(o.fog_color) - lum(f.fog_color)).abs() < 1e-4);
        assert!(o.fog_color[2] > o.fog_color[0]);
        assert!(o.sun_fog_color[0] > o.sun_fog_color[2]);
        assert!(o.half_dist < f.half_dist);
    }
}
