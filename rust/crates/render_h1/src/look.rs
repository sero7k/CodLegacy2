//! A map's look settings (its `vision/<map>.vision` and
//! `maps/createart/<map>_fog_hdr.gsc`) turned into the values MWR's post
//! shaders read.
//!
//! What each register means is read off the shaders themselves
//! (`postopaquefx_fog_hf`, `tonemap_linear`, `postfx_color2`). How the
//! engine fills them from these settings is code no file holds; the
//! mapping here follows the settings' meaning (a half-plane distance
//! halves transmittance, the three film tints are the curve's values at
//! dark, middle and light) and is to be checked against the game's
//! pictures. The fog's inputs as the game takes them (`G_SetFog`: start,
//! ln 2 / half-plane distance, colours, haze strength and spread,
//! extinction, in-scatter, distance scale, sky distance, sun direction,
//! height base, ln 2 / height half-plane, 100000 when height fog is off)
//! are from h1-mod's fog editor (GPL-3.0); how the renderer packs them
//! into the shader's registers is not.

use std::collections::HashMap;

use glam::{Mat4, Vec3, Vec4Swizzles};

use crate::code::{Camera, CodeValues};

/// `key value` lines (vision) or `ent.key = value;` lines (fog script).
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub values: HashMap<String, Vec<f32>>,
}

impl Settings {
    pub fn parse_vision(text: &str) -> Self {
        let mut values = HashMap::new();
        for line in text.lines() {
            let mut it = line.splitn(2, char::is_whitespace);
            let (Some(k), Some(v)) = (it.next(), it.next()) else {
                continue;
            };
            let v: Vec<f32> = v
                .trim()
                .trim_matches('"')
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            if !v.is_empty() {
                values.insert(k.to_ascii_lowercase(), v);
            }
        }
        Self { values }
    }

    /// Every `ent.key = value;` line of the script, later lines winning.
    pub fn parse_fog_script(text: &str) -> Self {
        Self::parse_fog_lines(text.lines())
    }

    /// The fog a map starts with: the script's block for the vision set
    /// named `vision_set` (`ent = maps\mp\_art::create_vision_set_fog(
    /// "<name>" );` starts a block). A `_fog_hdr.gsc` holds one block per
    /// vision set (the map's own, its interiors, `""` for the rest); the
    /// map's base vision set is the map name (`vision/<map>.vision`). Falls
    /// back to the `""` block, then the first. Taking every line made the
    /// last block (often `""`) win, and maps whose script the zone walk
    /// lost had none.
    pub fn parse_fog_script_for(text: &str, vision_set: &str) -> Self {
        let mut blocks: Vec<(String, Vec<&str>)> = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("//") {
                continue;
            }
            if let Some(i) = t.find("create_vision_set_fog") {
                let name = t[i..]
                    .split('"')
                    .nth(1)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                blocks.push((name, Vec::new()));
            } else if let Some((_, lines)) = blocks.last_mut() {
                lines.push(line);
            }
        }
        if blocks.is_empty() {
            return Self::parse_fog_script(text);
        }
        let want = vision_set.to_ascii_lowercase();
        let pick = blocks
            .iter()
            .find(|(n, _)| *n == want)
            .or_else(|| blocks.iter().find(|(n, _)| n.is_empty()))
            .unwrap_or(&blocks[0]);
        Self::parse_fog_lines(pick.1.iter().copied())
    }

    fn parse_fog_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Self {
        let mut values = HashMap::new();
        for line in lines {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("ent.") else {
                continue;
            };
            let Some((k, v)) = rest.split_once('=') else {
                continue;
            };
            let v = v
                .trim()
                .trim_end_matches(';')
                .trim_matches(|c| c == '(' || c == ')');
            let v: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if !v.is_empty() {
                values.insert(k.trim().to_ascii_lowercase(), v);
            }
        }
        Self { values }
    }

    /// A light set table: one row per setting, its name then its value
    /// (the first light set column).
    pub fn from_table(columns: usize, cells: &[String]) -> Self {
        let mut values = HashMap::new();
        for row in cells.chunks(columns.max(1)) {
            if let [k, v, ..] = row {
                let v: Vec<f32> = v
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                if !v.is_empty() {
                    values.insert(k.to_ascii_lowercase(), v);
                }
            }
        }
        Self { values }
    }

    pub fn f(&self, k: &str, default: f32) -> f32 {
        self.values
            .get(&k.to_ascii_lowercase())
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(default)
    }

    pub fn v3(&self, k: &str, default: [f32; 3]) -> Vec3 {
        match self.values.get(&k.to_ascii_lowercase()) {
            Some(v) if v.len() >= 3 => Vec3::new(v[0], v[1], v[2]),
            _ => Vec3::from(default),
        }
    }
}

/// The film pass (`postfx_color2`): `c = c * (base + delta L + quad L^2) +
/// bias` in display space, desaturated by `sat(delta.w L + base.w)` first.
///
/// The tint curve is the quadratic Bezier from the dark tint (L = 0) to the
/// light tint (L = 1) with the medium tint as its control point, scaled by
/// the contrast; the bias is brightness plus half of (1 - contrast). This
/// was Modern Warfare 2's mapping; MWR's exe does the same (read, 30 Oct:
/// `docs/H1_NATIVE.md`, "Film constants"), with `COLOR_BIAS.w` = 1 /
/// desaturation - 1 and three global dvars and the user's brightness at
/// neutral defaults.
/// The HDR film (`r_filmHDR*`) on MWR's film pass: its dark, medium and
/// specular tints as the pass's tint curve over brightness, no contrast,
/// brightness or desaturation change. MWR grades these over HDR luminance
/// between its crossovers; the pass grades over display brightness, the
/// nearest of MWR's own shaders.
pub fn film_hdr(vision: &Settings, code: &mut CodeValues) {
    let mut s = Settings::default();
    for (from, to) in [
        ("r_filmHDRDarkTint", "r_filmDarkTint"),
        ("r_filmHDRMediumTint", "r_filmMediumTint"),
        // Display white is HDR film's specular band (above its last
        // crossover): the sky and highlights keep the specular tint.
        ("r_filmHDRSpecularTint", "r_filmLightTint"),
    ] {
        s.values.insert(
            to.to_ascii_lowercase(),
            vision.v3(from, [1.0; 3]).to_array().to_vec(),
        );
    }
    s.values.insert("r_filmenable".into(), vec![1.0]);
    film(&s, code);
}

pub fn film(vision: &Settings, code: &mut CodeValues) {
    let on = vision.f("r_filmEnable", 0.0) != 0.0;
    let get = |k: &str, d: [f32; 3]| if on { vision.v3(k, d) } else { Vec3::from(d) };
    let (dark, mid, light) = (
        get("r_filmDarkTint", [1.0; 3]),
        get("r_filmMediumTint", [1.0; 3]),
        get("r_filmLightTint", [1.0; 3]),
    );
    let f = |k: &str, d: f32| if on { vision.f(k, d) } else { d };
    let (mut contrast, bright) = (f("r_filmContrast", 1.0), f("r_filmBrightness", 0.0));
    let (desat, desat_dark) = (
        f("r_filmDesaturation", 0.0),
        f("r_filmDesaturationDark", 0.0),
    );
    let mut bias = bright + 0.5 - 0.5 * contrast;
    if f("r_filmInvert", 0.0) != 0.0 {
        contrast = -contrast;
        bias += 1.0;
    }
    let base = dark * contrast;
    let delta = (mid - dark) * (2.0 * contrast);
    let quad = (light - mid * 2.0 + dark) * contrast;
    code.set_vec("COLOR_TINT_BASE", [base.x, base.y, base.z, desat_dark]);
    code.set_vec(
        "COLOR_TINT_DELTA",
        [delta.x, delta.y, delta.z, desat - desat_dark],
    );
    code.set_vec("COLOR_TINT_QUADRATIC_DELTA", [quad.x, quad.y, quad.z, 0.0]);
    code.set_vec("COLOR_BIAS", [bias, bias, bias, film_bias_w(desat)]);
}

/// `COLOR_BIAS.w` as the engine sets it: `1 / max(desaturation, 2^-12) - 1`
/// (read; `postfx_color2` does not read it, other film variants may).
pub fn film_bias_w(desaturation: f32) -> f32 {
    1.0 / desaturation.max(1.0 / 4096.0) - 1.0
}

/// The atmospheric fog (`postopaquefx_fog_hf`, and the forward-fogged
/// shaders): registers P0..P6 as the shader reads them.
///
/// * P0: Rayleigh (haze) colour, density per unit (ln 2 / half plane).
/// * P1: Mie (sun fog) colour, phase g.
/// * P2: extinction per channel, max opacity.
/// * P3: in-scatter strength, start offset (-start x density).
/// * P4: sun direction in view space, sky distance.
/// * P5: z height falloff (0: off), w distance scale.
/// * P6: up in view space, the sky's max opacity.
pub fn fog(fog: &Settings, view: &Camera, code: &mut CodeValues) {
    let on = fog.f("atmosFogEnabled", 0.0) != 0.0;
    let rot: Mat4 = view.rotation;
    let to_view = |v: Vec3| (rot * v.extend(0.0)).xyz();
    let half = fog.f("atmosFogHalfPlaneDistance", 8000.0).max(1.0);
    let density = if on {
        std::f32::consts::LN_2 / half
    } else {
        0.0
    };
    let start = fog.f("atmosFogStartDistance", 0.0);
    let haze = fog.v3("atmosFogHazeColor", [0.0; 3]);
    let sun_col = fog.v3("atmosFogSunFogColor", [0.0; 3]);
    // Mie: the shader's phase is (1 - k^2) / (4 pi (1 + P1.w cos)^2), the
    // Schlick form with P1.w = -k; the haze spread is how far from the sun
    // the glow reaches (g = 1 - spread), the haze strength its amount.
    // P1.w as the exe's fog builder writes it: `min(2 spread - 1, 0.999)`
    // (`docs/H1_PARITY_PROGRESS.md`, fog). The fitted Schlick k went past 1
    // for a small spread (Shipment's 0.0018), turning the sun's glow into a
    // black disc; `BO2MP_H1_FOG_PHASE=fitted` keeps it.
    let spread = fog.f("atmosFogHazeSpread", 0.25);
    let k = if std::env::var("BO2MP_H1_FOG_PHASE").is_ok_and(|v| v == "fitted") {
        let g = (1.0 - spread).clamp(0.0, 0.999);
        1.55 * g - 0.55 * g * g * g
    } else {
        -(2.0 * spread - 1.0).min(0.999)
    };
    let haze_strength = fog.f("atmosFogHazeStrength", 0.5);
    let ext = fog.f("atmosFogExtinctionStrength", 1.0);
    let ins = fog.f("atmosFogInScatterStrength", 1.0);
    let sun = to_view(
        fog.v3("atmosFogSunDirection", [0.0, 0.0, 1.0])
            .normalize_or_zero(),
    );
    let up = to_view(Vec3::Z);
    // Height falloff per unit. Off is a tiny falloff, not zero: both fog
    // shaders take their height path whenever the height along the ray is
    // not small and divide by it there, and (1 - e^-h) / h tends to 1.
    let height = if fog.f("atmosFogHeightFogEnabled", 0.0) != 0.0 {
        std::f32::consts::LN_2 / fog.f("atmosFogHeightFogHalfPlaneDistance", 1000.0).max(1.0)
    } else {
        // Off: h1-mod's fog editor passes the game ln 2 / 100000.
        std::f32::consts::LN_2 / 100_000.0
    };
    code.set_vec("ATMOS_FOG_PARMS_0", [haze.x, haze.y, haze.z, density]);
    let mie = sun_col * haze_strength;
    code.set_vec("ATMOS_FOG_PARMS_1", [mie.x, mie.y, mie.z, -k]);
    code.set_vec(
        "ATMOS_FOG_PARMS_2",
        [ext, ext, ext, if on { 1.0 } else { 0.0 }],
    );
    code.set_vec("ATMOS_FOG_PARMS_3", [ins, ins, ins, -start * density]);
    // The sky (depth 0) takes P4.w as its whole fog amount (the shader
    // uses it where a surface uses density x distance + P3.w): the fog
    // over the sky distance.
    // (With no atmospheric fog this must still be a distance: the shader
    // normalises the sky's view ray scaled by it, and a zero gives NaN,
    // which drew every sky black on maps without a fog script. The game's
    // fog string default is 100000.)
    let sky = if on {
        ((fog.f("atmosFogSkyDistance", 30000.0) - start) * density).max(0.0)
    } else {
        100_000.0
    };
    code.set_vec("ATMOS_FOG_PARMS_4", [sun.x, sun.y, sun.z, sky]);
    code.set_vec(
        "ATMOS_FOG_PARMS_5",
        [0.0, 0.0, height, fog.f("atmosFogDistanceScale", 1.0)],
    );
    code.set_vec(
        "ATMOS_FOG_PARMS_6",
        [up.x, up.y, up.z, if on { 1.0 } else { 0.0 }],
    );
    // The view ray at depth 1 from the clip position, and the near plane.
    code.set_vec("INV_SCENE_PROJECTION", [view.tan_x, 0.0, view.tan_y, 0.0]);
    code.set_vec("ZNEAR", [view.near, 0.0, 0.0, 0.0]);
}

/// The atmospheric fog constants as MWR's fog builder writes them
/// (`docs/H1_PARITY_PROGRESS.md`, fog; read from the exe): the map's
/// `_fog_hdr` values reach the builder through the game's fog string
/// (start, ln 2 / half distance, colours as sRGB bytes, haze strength and
/// spread, in-scatter as an EV, extinction, distance scale, sky distance,
/// sky falloff angles, sun direction, height base and ln 2 / height half
/// distance). `stage_scale`: the eye's stage radiometric scale. P4 is
/// written in world space here and P4/P6 are rotated into the view by
/// [`fog`] as the exe's later fog writer does; this overwrites P0..P3, P5.
pub fn fog_exact(fog: &Settings, view: &Camera, stage_scale: f32, code: &mut CodeValues) {
    if fog.f("atmosFogEnabled", 0.0) == 0.0 {
        return;
    }
    // Colours travel as bytes (the parser packs them), then sRGB to linear.
    let colour = |key: &str| {
        let c = fog.v3(key, [0.0; 3]);
        let lin = |v: f32| {
            let v = (v.clamp(0.0, 1.0) * 255.0).round() / 255.0;
            if v > 0.03928 {
                ((v + 0.055) / 1.055).powf(2.4)
            } else {
                v / 12.92
            }
        };
        Vec3::new(lin(c.x), lin(c.y), lin(c.z))
    };
    let sun_fog = colour("atmosFogSunFogColor");
    let haze = colour("atmosFogHazeColor");
    let a = fog.f("atmosFogHazeStrength", 0.5);
    let spread = fog.f("atmosFogHazeSpread", 0.25);
    let density = std::f32::consts::LN_2 / fog.f("atmosFogHalfPlaneDistance", 5000.0).max(1.0);
    let ins = fog.f("atmosFogInScatterStrength", 0.0);
    let r = if ins != -8.0 {
        2f32.powf(ins - 2.84) * stage_scale
    } else {
        1.0
    };
    let p0 = sun_fog * ((1.0 - a) * density);
    let p1 = haze * (a * density);
    let ext = (p0 + p1).max(Vec3::splat(1e-9));
    code.set_vec(
        "ATMOS_FOG_PARMS_0",
        [p0.x, p0.y, p0.z, fog.f("atmosFogDistanceScale", 1.0)],
    );
    code.set_vec(
        "ATMOS_FOG_PARMS_1",
        [p1.x, p1.y, p1.z, (2.0 * spread - 1.0).min(0.999)],
    );
    code.set_vec(
        "ATMOS_FOG_PARMS_2",
        [ext.x, ext.y, ext.z, fog.f("atmosFogExtinctionStrength", 1.0)],
    );
    code.set_vec(
        "ATMOS_FOG_PARMS_3",
        [
            r / ext.x,
            r / ext.y,
            r / ext.z,
            -fog.f("atmosFogStartDistance", 0.0),
        ],
    );
    // The sky's angular falloff (off: start -90, range 9000, as the game's
    // fog string sends it).
    let (start_deg, range_deg) = if fog.f("atmosFogSkyAngularFalloffEnabled", 0.0) != 0.0 {
        (
            fog.f("atmosFogSkyFalloffStartAngle", -90.0),
            fog.f("atmosFogSkyFalloffAngleRange", 90.0),
        )
    } else {
        (-90.0, 9000.0)
    };
    let deg = std::f32::consts::PI / 180.0;
    let quarter = std::f32::consts::FRAC_PI_2;
    let q = (range_deg * deg / quarter).floor();
    let w = (((range_deg * deg) - q * quarter).sin() + q).max(0.001);
    let t = (start_deg * deg).sin();
    let height_density = if fog.f("atmosFogHeightFogEnabled", 0.0) != 0.0 {
        std::f32::consts::LN_2 / fog.f("atmosFogHeightFogHalfPlaneDistance", 100000.0).max(1.0)
    } else {
        std::f32::consts::LN_2 / 100_000.0
    };
    let base = if fog.f("atmosFogHeightFogEnabled", 0.0) != 0.0 {
        fog.f("atmosFogHeightFogBaseHeight", 0.0)
    } else {
        0.0
    };
    code.set_vec(
        "ATMOS_FOG_PARMS_5",
        [
            1.0 / w,
            -(t / w),
            height_density,
            (-(view.eye.z - base).max(0.0) * height_density).exp(),
        ],
    );
    // P4: the sun direction (normalised by the parser) rotated into the
    // view, with the raw sky distance.
    let sun = (view.rotation
        * fog
            .v3("atmosFogSunDirection", [0.0, 0.0, 1.0])
            .normalize_or_zero()
            .extend(0.0))
    .xyz();
    code.set_vec(
        "ATMOS_FOG_PARMS_4",
        [sun.x, sun.y, sun.z, fog.f("atmosFogSkyDistance", 100000.0)],
    );
    // P6: world up in the view; .w as the exe's per-view fog writer sets
    // it: P2.w (the fog's extinction strength, its max opacity) times a
    // float dvar (default 1, range 0..1). Was 1.
    let up = (view.rotation * Vec3::Z.extend(0.0)).xyz();
    let w = if std::env::var("BO2MP_H1_FOG_SKY_OPACITY").is_ok_and(|v| v == "one") {
        1.0
    } else {
        fog.f("atmosFogExtinctionStrength", 1.0) * SKY_OPACITY_SCALE
    };
    code.set_vec("ATMOS_FOG_PARMS_6", [up.x, up.y, up.z, w]);
}

/// Whether a fog script's fog is the legacy (distance) fog: the map's
/// atmospheric fog is off and the legacy fog has an opacity (the exe's
/// fog builder takes its legacy branch when the atmospheric flag is clear;
/// the post-opaque pick then takes the `dfog` materials).
pub fn legacy_fog_on(fog: &Settings) -> bool {
    fog.f("atmosFogEnabled", 1.0) == 0.0
        && !fog.values.is_empty()
        && fog.f("maxOpacity", 0.0) > 0.0
        && std::env::var("BO2MP_H1_LEGACY_FOG").map_or(true, |v| v != "0")
}

/// sRGB bytes (as the fog string packs colours) to linear.
fn srgb_byte_to_linear(c: Vec3) -> Vec3 {
    let lin = |v: f32| {
        let v = (v.clamp(0.0, 1.0) * 255.0).round() / 255.0;
        if v > 0.03928 {
            ((v + 0.055) / 1.055).powf(2.4)
        } else {
            v / 12.92
        }
    };
    Vec3::new(lin(c.x), lin(c.y), lin(c.z))
}

/// The fog builder's intensity conversion (an EV, -8 meaning 1), as for
/// the atmospheric in-scatter: `2^(ev - 2.84) x stage scale`.
fn fog_intensity(ev: f32, stage_scale: f32) -> f32 {
    if ev != -8.0 {
        2f32.powf(ev - 2.84) * stage_scale
    } else {
        1.0
    }
}

/// The legacy fog constants (`FOG`, `FOG_COLOR_*`, `FOG_SUN_*`,
/// `FOG_SKY_FOG`, `FOG_HEIGHT_FOG`, `FOG_SKY_DIR`, `FOG_DEPTHHACK`) as the
/// exe's fog builder writes them in its legacy branch (read; see
/// `docs/H1_NATIVE.md`, "Legacy fog"), from the `_fog_hdr` values. How the
/// script's keys map onto the builder's fog-state fields is INFERRED from
/// the fog string's order and the `postopaquefx_dfog` shader's use of each
/// value (start, max opacity, density ln 2 / halfway distance, the sun fog
/// fade angles and scale, the sky fog angles and intensity, the height fog
/// falloff), not read from the client's string parser.
pub fn fog_legacy(fog: &Settings, view: &Camera, stage_scale: f32, code: &mut CodeValues) {
    let start = fog.f("startDist", 0.0);
    let density = std::f32::consts::LN_2 / fog.f("halfwayDist", 8000.0).max(1.0);
    let max_opacity = fog.f("maxOpacity", 1.0);
    let colour = Vec3::new(fog.f("red", 1.0), fog.f("green", 1.0), fog.f("blue", 1.0));
    let gamma = colour.clamp(Vec3::ZERO, Vec3::ONE);
    let lin = srgb_byte_to_linear(colour) * fog_intensity(fog.f("HDRColorIntensity", -8.0), stage_scale);
    code.set_vec("FOG", [0.0, 1.0 - max_opacity, -density, density * start]);
    code.set_vec("FOG_DEPTHHACK", [0.0, 1.0 - max_opacity, -density, density * start]);
    code.set_vec("FOG_COLOR_LINEAR", [lin.x, lin.y, lin.z, 1.0]);
    code.set_vec("FOG_COLOR_GAMMA", [gamma.x, gamma.y, gamma.z, 1.0]);
    // Sun fog: off, the sun colour is the fog colour and its scale 1 (no
    // change across the sun's fade).
    let sun_on = fog.f("sunFogEnabled", 0.0) != 0.0;
    let (sun_lin, sun_gamma, scale) = if sun_on {
        let c = Vec3::new(fog.f("sunRed", 1.0), fog.f("sunGreen", 1.0), fog.f("sunBlue", 1.0));
        (
            srgb_byte_to_linear(c) * fog_intensity(fog.f("HDRSunColorIntensity", -8.0), stage_scale),
            c.clamp(Vec3::ZERO, Vec3::ONE),
            fog.f("normalFogScale", 1.0),
        )
    } else {
        (lin, gamma, 1.0)
    };
    let deg = std::f32::consts::PI / 180.0;
    let cos_begin = (fog.f("sunBeginFadeAngle", 0.0) * deg).cos();
    let cos_end = (fog.f("sunEndFadeAngle", 180.0) * deg).cos();
    let fade = if cos_end < cos_begin {
        1.0 / (cos_begin - cos_end)
    } else {
        1.0e6
    };
    code.set_vec(
        "FOG_SUN_CONSTS",
        [-density * scale, cos_end, fade, -density],
    );
    code.set_vec("FOG_SUN_COLOR_LINEAR", [sun_lin.x, sun_lin.y, sun_lin.z, 1.0]);
    code.set_vec("FOG_SUN_COLOR_GAMMA", [sun_gamma.x, sun_gamma.y, sun_gamma.z, 1.0]);
    let to_view = |v: Vec3| (view.rotation * v.extend(0.0)).xyz();
    let sun = to_view(fog.v3("sunDir", [1.0, 0.0, 0.0]).normalize_or_zero());
    code.set_vec("FOG_SUN_DIR", [sun.x, sun.y, sun.z, 0.0]);
    let up = to_view(Vec3::Z);
    code.set_vec("FOG_SKY_DIR", [up.x, up.y, up.z, 0.0]);
    let min = fog.f("skyFogMinAngle", 0.0) * deg;
    let mut max = fog.f("skyFogMaxAngle", 0.0) * deg;
    if max <= min {
        max = min + 1.0e-3;
    }
    code.set_vec(
        "FOG_SKY_FOG",
        [min, 1.0 / (max - min), fog.f("skyFogIntensity", 0.0) * max_opacity, 0.0],
    );
    let (falloff, base) = if fog.f("heightFogEnabled", 0.0) != 0.0 {
        (
            std::f32::consts::LN_2 / fog.f("heightFogHalfPlaneDistance", 100000.0).max(1.0),
            fog.f("heightFogBaseHeight", 0.0),
        )
    } else {
        (std::f32::consts::LN_2 / 100_000.0, 0.0)
    };
    code.set_vec(
        "FOG_HEIGHT_FOG",
        [
            -start,
            (-(view.eye.z - base).max(0.0) * falloff).exp(),
            falloff,
            0.0,
        ],
    );
    // The atmospheric pass's opacities at zero so its variants copy.
    code.set_vec("ATMOS_FOG_PARMS_6", [up.x, up.y, up.z, 0.0]);
}

/// The post-opaque pass the exe picks for a view (read; its material table
/// holds `postopaquefx_` + `[ssaolq_|ssaohq_]` + `fog|dfog` + `[_hf]` +
/// `[_fsf]`, indexed by `ssao | 4 (atmospheric fog) | 0x10 (height fog) |
/// 0x20 (sky falloff)`). For atmospheric fog: drawn when the fog's max
/// opacity (P2.w) is above 0; `_hf` when ln 2 / height density (the height
/// half-plane distance) is below 99000; `_fsf` when the sky falloff angle
/// range is below 8910 degrees (the fog string sends 9000 when the
/// falloff is off). `ssao`: 0 off, 1 low, 2 high. `None`: no pass.
/// Returns the technique's name.
pub fn post_opaque_technique(fog: &Settings, ssao: u32) -> Option<String> {
    let on = fog.f("atmosFogEnabled", 0.0) != 0.0
        && fog.f("atmosFogExtinctionStrength", 1.0) > 0.0;
    let ssao = match ssao {
        1 => "ssaolq_",
        2 | 3 => "ssaohq_",
        _ => "",
    };
    // Legacy fog (the `dfog` materials): `_hf` when ln 2 / its height
    // falloff is below 99000; no sky falloff variant.
    if legacy_fog_on(fog) {
        let half = if fog.f("heightFogEnabled", 0.0) != 0.0 {
            fog.f("heightFogHalfPlaneDistance", 100000.0).max(1.0)
        } else {
            100_000.0
        };
        let hf = if half < 99_000.0 { "_hf" } else { "" };
        return Some(format!("postopaquefx_{ssao}dfog{hf}"));
    }
    if !on {
        return (!ssao.is_empty()).then(|| format!("postopaquefx_{}", ssao.trim_end_matches('_')));
    }
    let height_half = if fog.f("atmosFogHeightFogEnabled", 0.0) != 0.0 {
        fog.f("atmosFogHeightFogHalfPlaneDistance", 100000.0).max(1.0)
    } else {
        100_000.0
    };
    let range = if fog.f("atmosFogSkyAngularFalloffEnabled", 0.0) != 0.0 {
        fog.f("atmosFogSkyFalloffAngleRange", 90.0)
    } else {
        9000.0
    };
    let hf = if height_half < 99_000.0 { "_hf" } else { "" };
    // (The `_fsf` materials' technique is named `_flf` in the zone.)
    let fsf = if range < 8910.0 { "_flf" } else { "" };
    Some(format!("postopaquefx_{ssao}fog{hf}{fsf}"))
}

/// The exe's sky fog opacity scale dvar's default (registered with
/// default 1, range 0..1, read by the fog writer and the fog builder's
/// sun-fog branch).
pub const SKY_OPACITY_SCALE: f32 = 1.0;

/// The filmic tone map's own buffer (`b4` of `tonemap_filmic`): the
/// shader takes the scene colour c and returns
/// `saturate((A c + B) / (C c + D)) ^ gamma`, with one set of row
/// coefficients below the crossover `[0].x` (row 1: A, C, B, D in x, y,
/// z, w) and another above it (row 2), and gamma in `[3].z`.
///
/// How the engine derives the coefficients from the light set's white,
/// shoulder, crossover, toe and black is code no file holds. This uses
/// one rational shoulder, `e c / (1 + k e c)`, for both segments, its
/// exposure from the light set's EV (one stop per EV, 0.70 at EV 10) and
/// k = 0.10: the curve fitted to HorizonMW's own picture of Rust (per
/// channel quantiles of our HDR scene against its displayed colours).
pub fn filmic_buffer(lightset: &Settings, scale: f32) -> Vec<[f32; 4]> {
    let ev = lightset.f("tonemapexposure", 10.0) + lightset.f("tonemapexposureadjust", 0.0);
    let e = FILMIC_EXPOSURE_AT_EV10 * 2f32.powf(10.0 - ev) * scale;
    let k = FILMIC_SHOULDER;
    let row = [e, k * e, 0.0, 1.0];
    vec![[f32::MAX, 0.0, 0.0, 0.0], row, row, [0.0, 0.0, 1.0, 0.0]]
}

/// Fitted on Rust against HorizonMW (see `filmic_buffer`).
pub const FILMIC_EXPOSURE_AT_EV10: f32 = 0.70;
pub const FILMIC_SHOULDER: f32 = 0.10;

/// The tone map's own buffer (`b4` of `tonemap_*`): row 3 is (exposure,
/// -, gamma, -); the linear curve is `saturate(c x exposure)^gamma`.
pub fn tonemap_buffer(exposure: f32, gamma: f32) -> Vec<[f32; 4]> {
    vec![[0.0; 4], [0.0; 4], [0.0; 4], [exposure, 0.0, gamma, 0.0]]
}

/// bo2mp: the `setdvar( "name", value )` calls with a number in a map's
/// script (`maps/mp/<map>.gsc`; a later call wins), by lower-case name.
/// Horizon MW's map scripts set the engine's lighting tweaks there (light
/// grid intensity and contrast, colour scales, the sun's tweak strength),
/// which the light set does not hold.
pub fn script_dvars(text: &str) -> std::collections::HashMap<String, f32> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let Some(at) = lower.find("setdvar(") else { continue };
        if lower[..at].ends_with('_') || lower[..at].chars().last().is_some_and(char::is_alphanumeric) {
            continue; // (setdvar_cg_ng, ...)
        }
        let args = &line[at + "setdvar(".len()..];
        let Some(end) = args.rfind(')') else { continue };
        let mut parts = args[..end].splitn(2, ',');
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else { continue };
        let name = name.trim().trim_matches('"').to_ascii_lowercase();
        let value = value.trim().trim_matches('"').trim();
        if name.is_empty() || name.contains(' ') {
            continue;
        }
        if let Ok(v) = value.parse::<f32>() {
            out.insert(name, v);
        }
    }
    out
}

/// bo2mp: what a map script's lighting tweaks change, each only under its
/// engine switch (as the engine reads them: a tweak value overrides the
/// light set only while its `...UseTweaks` / `...EnableTweaks` is on):
/// `r_colorScaleUseTweaks` -> the light set's diffuse/specular colour
/// scales; `r_tonemapUseTweaks` -> its tone map values; the sun's tweak
/// strengths (`r_primaryLightUseTweaks`) go to the vision settings, where
/// `light::ColorScales` reads them. Returns what was applied, for the log.
pub fn apply_script_dvars(script: &std::collections::HashMap<String, f32>, lightset: &mut Settings, vision: &mut Settings) -> Vec<String> {
    let mut done = Vec::new();
    let on = |k: &str| script.get(k).is_some_and(|v| *v != 0.0);
    if on("r_colorscaleusetweaks") {
        for k in ["diffusecolorscale", "specularcolorscale"] {
            if let Some(v) = script.get(&format!("r_{k}")) {
                lightset.values.insert(k.to_owned(), vec![*v]);
                done.push(format!("{k} {v}"));
            }
        }
    }
    if on("r_tonemapusetweaks") {
        for (k, v) in script.iter().filter(|(k, _)| k.starts_with("r_tonemap") && k.as_str() != "r_tonemapusetweaks") {
            let key = k.trim_start_matches("r_").to_owned();
            lightset.values.insert(key.clone(), vec![*v]);
            done.push(format!("{key} {v}"));
        }
    }
    // (HMW's map scripts set the veil the same way: `r_veilUseTweaks` with
    // `r_veil`, `r_veilStrength`, `r_veilBackgroundStrength`; Terminal 0.06 /
    // 0.85. The engine's veil tweak toggle takes the dvars over the light
    // set's, as the colour scales.)
    if on("r_veilusetweaks") {
        for (from, to) in [("r_veil", "veil"), ("r_veilstrength", "veilstrength"), ("r_veilbackgroundstrength", "veilbackgroundstrength")] {
            if let Some(v) = script.get(from) {
                lightset.values.insert(to.to_owned(), vec![*v]);
                done.push(format!("{to} {v}"));
            }
        }
    }
    if on("r_primarylightusetweaks") {
        for k in ["r_primarylightusetweaks", "r_primarylighttweakdiffusestrength", "r_primarylighttweakspecularstrength"] {
            if let Some(v) = script.get(k) {
                vision.values.insert(k.to_owned(), vec![*v]);
                done.push(format!("{k} {v}"));
            }
        }
    }
    done
}

/// bo2mp: the light grid's intensity tweak from a map script (the models'
/// baked light times it), 1 unless `r_lightGridEnableTweaks` is on.
/// (`r_lightGridContrast` is not applied: its maths is not read yet.)
pub fn script_grid_intensity(script: &std::collections::HashMap<String, f32>) -> f32 {
    if script.get("r_lightgridenabletweaks").is_some_and(|v| *v != 0.0) {
        script.get("r_lightgridintensity").copied().unwrap_or(1.0).clamp(0.0, 4.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod script_tests {
    use super::*;

    const ALPHA: &str = "main()\n{\n    maps\\mp\\mp_alpha_precache::main();\n    setdvar( \"r_lightGridEnableTweaks\", 1 );\n    setdvar( \"r_lightGridIntensity\", 1.33 );\n\tsetdvar(\"r_specularColorScale\", 2);\n\tsetdvar(\"r_diffuseColorScale\", 2);\n    setdvar(\"r_tonemapAuto\", \"1\");\n    setdvar(\"r_tonemapUseTweaks\", \"0\");\n\tsetdvar(\"r_colorScaleUseTweaks\", \"1\");\n    // setdvar( \"r_ssao\", 9 );\n    setdvar_cg_ng( var_0, var_1, var_2 )\n}\n";

    #[test]
    fn a_map_script_s_lighting_tweaks_apply_under_their_switches() {
        let d = script_dvars(ALPHA);
        assert_eq!(d.get("r_lightgridintensity"), Some(&1.33));
        assert!(!d.contains_key("r_ssao"));
        let (mut ls, mut vision) = (Settings::default(), Settings::default());
        ls.values.insert("tonemapauto".into(), vec![0.0]);
        let done = apply_script_dvars(&d, &mut ls, &mut vision);
        assert_eq!(ls.f("diffusecolorscale", 1.0), 2.0);
        assert_eq!(ls.f("specularcolorscale", 1.0), 2.0);
        // the tone map tweaks are off (r_tonemapUseTweaks 0): the light set's stays
        assert_eq!(ls.f("tonemapauto", 9.0), 0.0);
        assert_eq!(done.len(), 2);
        assert!((script_grid_intensity(&d) - 1.33).abs() < 1e-6);
        // no switch: nothing
        assert_eq!(script_grid_intensity(&script_dvars("setdvar( \"r_lightGridIntensity\", 2 );")), 1.0);
    }
}

#[cfg(test)]
mod fog_tests {
    use super::*;

    const SCRIPT: &str = "main()\n{\n\tent = maps\\mp\\_art::create_vision_set_fog( \"mp_x\" );\n\
        \tent.atmosFogEnabled = 1;\n\tent.atmosFogHalfPlaneDistance = 100;\n\
        \tent.atmosFogSkyAngularFalloffEnabled = 1;\n\tent.atmosFogSkyFalloffAngleRange = 140;\n\
        \tent = maps\\mp\\_art::create_vision_set_fog( \"\" );\n\tent.atmosFogHalfPlaneDistance = 200;\n}\n";

    #[test]
    fn picks_the_map_vision_block() {
        let f = Settings::parse_fog_script_for(SCRIPT, "mp_x");
        assert_eq!(f.f("atmosFogHalfPlaneDistance", 0.0), 100.0);
        let f = Settings::parse_fog_script_for(SCRIPT, "mp_other");
        assert_eq!(f.f("atmosFogHalfPlaneDistance", 0.0), 200.0);
        assert_eq!(Settings::parse_fog_script(SCRIPT).f("atmosFogHalfPlaneDistance", 0.0), 200.0);
    }

    #[test]
    fn post_opaque_pick() {
        let f = Settings::parse_fog_script_for(SCRIPT, "mp_x");
        assert_eq!(post_opaque_technique(&f, 0).as_deref(), Some("postopaquefx_fog_flf"));
        assert_eq!(post_opaque_technique(&f, 2).as_deref(), Some("postopaquefx_ssaohq_fog_flf"));
        let mut g = f.clone();
        g.values.insert("atmosfogheightfogenabled".into(), vec![1.0]);
        g.values.insert("atmosfogheightfoghalfplanedistance".into(), vec![500.0]);
        g.values.insert("atmosfogskyangularfalloffenabled".into(), vec![0.0]);
        assert_eq!(post_opaque_technique(&g, 0).as_deref(), Some("postopaquefx_fog_hf"));
        assert_eq!(post_opaque_technique(&Settings::default(), 0), None);
        assert_eq!(post_opaque_technique(&Settings::default(), 1).as_deref(), Some("postopaquefx_ssaolq"));
        let mut l = Settings::default();
        for (k, v) in [("atmosfogenabled", 0.0), ("maxopacity", 0.9), ("heightfogenabled", 1.0), ("heightfoghalfplanedistance", 400.0)] {
            l.values.insert(k.into(), vec![v]);
        }
        assert_eq!(post_opaque_technique(&l, 0).as_deref(), Some("postopaquefx_dfog_hf"));
        let mut c = CodeValues::default();
        let cam = crate::code::Camera {
            eye: Vec3::ZERO,
            rotation: Mat4::IDENTITY,
            view_projection: Mat4::IDENTITY,
            tan_x: 1.0,
            tan_y: 1.0,
            near: 1.0,
        };
        l.values.insert("startdist".into(), vec![100.0]);
        l.values.insert("halfwaydist".into(), vec![1000.0]);
        fog_legacy(&l, &cam, 0.001, &mut c);
        let fog = c.row(crate::code::code_index("FOG"), 0);
        assert!((fog[1] - 0.1).abs() < 1e-6 && (fog[2] + std::f32::consts::LN_2 / 1000.0).abs() < 1e-7);
        assert!((fog[3] - 100.0 * std::f32::consts::LN_2 / 1000.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod film_tests {
    use super::*;
    use crate::code::code_index;

    /// The engine's film step (quadratic mode, its default; the global
    /// contrast/brightness/desaturation dvars and the user brightness at
    /// their neutral defaults) and `film` agree.
    #[test]
    fn film_matches_engine_mapping() {
        let mut v = Settings::default();
        for (k, val) in [
            ("r_filmenable", vec![1.0]),
            ("r_filmcontrast", vec![1.3]),
            ("r_filmbrightness", vec![0.05]),
            ("r_filmdesaturation", vec![0.2]),
            ("r_filmdesaturationdark", vec![0.1]),
            ("r_filmdarktint", vec![0.9, 0.95, 1.1]),
            ("r_filmmediumtint", vec![1.0, 1.0, 1.0]),
            ("r_filmlighttint", vec![1.1, 1.05, 0.9]),
        ] {
            v.values.insert(k.into(), val);
        }
        let mut c = CodeValues::default();
        film(&v, &mut c);
        let row = |n: &str| c.row(code_index(n), 0);
        // Engine: k = contrast; base = k D, delta = 2k M - 2k D,
        // quad = k D - 2k M + k L; bias = b + 0.5 - 0.5 contrast.
        let k = 1.3f32;
        let (d, m, l) = ([0.9, 0.95, 1.1], [1.0f32; 3], [1.1, 1.05, 0.9]);
        for i in 0..3 {
            assert!((row("COLOR_TINT_BASE")[i] - k * d[i]).abs() < 1e-6);
            assert!((row("COLOR_TINT_DELTA")[i] - (2.0 * k * m[i] - 2.0 * k * d[i])).abs() < 1e-6);
            let q = k * d[i] - 2.0 * k * m[i] + k * l[i];
            assert!((row("COLOR_TINT_QUADRATIC_DELTA")[i] - q).abs() < 1e-6);
        }
        assert!((row("COLOR_BIAS")[0] - (0.05 + 0.5 - 0.65)).abs() < 1e-6);
        assert!((row("COLOR_TINT_BASE")[3] - 0.1).abs() < 1e-6);
        assert!((row("COLOR_TINT_DELTA")[3] - 0.1).abs() < 1e-6);
        assert!((row("COLOR_BIAS")[3] - 4.0).abs() < 1e-5);
        // Film off: neutral, and the desaturation floor.
        let mut c = CodeValues::default();
        film(&Settings::default(), &mut c);
        assert_eq!(c.row(code_index("COLOR_BIAS"), 0), [0.0, 0.0, 0.0, 4095.0]);
    }
}
