//! The lighting code constants as MWR's renderer fills them (read from the
//! game's executable, `docs/H1_NATIVE.md`, "Lighting"), next to the fitted
//! values used before, behind `BO2MP_H1_LIGHT=exact|fitted`.
//!
//! * **Primary light (the sun).** The scene's light is the map's
//!   `ComPrimaryLight` copied as it is (colour unscaled). Per draw the
//!   engine sets `LIGHT_POSITION = (dir, 0)`, `LIGHT_DIFFUSE = (colour x
//!   diffuse scale, 1)` and `LIGHT_SPECULAR = (colour x specular scale, 1)`.
//!   The scales are the light set's `diffusecolorscale` and
//!   `specularcolorscale` (1 when the map has no light set: the defaults
//!   come from the two tweak dvars' defaults, both 1), times the vision's
//!   `r_primaryLightTweakDiffuseStrength` / `...SpecularStrength` when its
//!   `r_primaryLightUseTweaks` is on.
//! * **Fitted** (before): colour x pi for both, fitted to HorizonMW's picture
//!   of Rust. The exe has no pi; the two differ by that factor.
//! * **`LIGHT_PROBE_AMBIENT`** of a static model: its ground-lighting value
//!   (four halves in the model's lighting record) when the model's flags
//!   carry 0x20, as is. Only the `lprobe` techniques read it.

use glam::Vec3;

use crate::look::Settings;

/// Which lighting maths the native MWR path uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightMath {
    /// As read from the exe.
    Exact,
    /// The values fitted to HorizonMW's pictures before the exe was read.
    Fitted,
}

impl LightMath {
    pub fn parse(v: &str) -> Self {
        if v.trim().eq_ignore_ascii_case("fitted") {
            Self::Fitted
        } else {
            Self::Exact
        }
    }

    /// `BO2MP_H1_LIGHT`; default exact (the game's own maths, owner's
    /// decision 30 Oct); `fitted` brings back the earlier pi-scaled sun.
    pub fn from_env() -> Self {
        std::env::var("BO2MP_H1_LIGHT").map_or(Self::Exact, |v| Self::parse(&v))
    }
}

/// The fitted factor on the primary light's colour (HorizonMW's Rust).
pub const FITTED_SUN_SCALE: f32 = std::f32::consts::PI;

/// The diffuse and specular scales the engine applies to every primary
/// light's colour: the light set's, times the vision's primary light
/// tweaks when those are on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorScales {
    pub diffuse: f32,
    pub specular: f32,
}

impl ColorScales {
    pub fn read(lightset: &Settings, vision: &Settings) -> Self {
        let mut s = Self {
            diffuse: lightset.f("diffusecolorscale", 1.0),
            specular: lightset.f("specularcolorscale", 1.0),
        };
        if vision.f("r_primaryLightUseTweaks", 0.0) != 0.0 {
            s.diffuse *= vision.f("r_primaryLightTweakDiffuseStrength", 1.0);
            s.specular *= vision.f("r_primaryLightTweakSpecularStrength", 1.0);
        }
        s
    }
}

/// Whether the old-style sun lift applies ([`usermap_lift`]). OFF by default
/// (9 Nov 2026): the exe's world shader has no such factor, and against the
/// owner's HMW screenshot of Terminal from the same spot the lift left the
/// sky about 3 times too dark and the sunlit ground blown out (it raised the
/// world about 10 times against a sky drawn at the stage scale, and the
/// metered exposure then pulled the whole frame down). The exposure stays
/// metered for old fixed sets ([`meters_old_fixed`]). `BO2MP_H1_GUESSES=1`
/// brings the lift back.
pub fn guesses() -> bool {
    std::env::var("BO2MP_H1_GUESSES").is_ok_and(|v| v.trim() == "1")
}

/// The factor on the baked secondary lightmap (the shade, the sky and bounce
/// light): the old-style sun lift only ([`usermap_lift`], stored by the scene
/// as `h1sunlift`), so sun and shade rise together. The light set's own
/// `diffusecolorscale` (and the vision's sun tweak) scale the sun alone, as in
/// the exe's lit shaders (read 9 Nov 2026: sun colour x N.L, the baked light
/// untouched); applying it to the shade too washed out maps with an authored
/// scale (Trailer Park 3.17: lavender shade, white sun). A kept look
/// (`keeplook`) keeps the old whole-scale factor.
/// `BO2MP_H1_LM_SCALE=<n>` sets it, `off` (or 1) keeps the page as baked.
pub fn secondary_scale(lightset: &Settings, vision: &Settings) -> f32 {
    if let Ok(v) = std::env::var("BO2MP_H1_LM_SCALE") {
        let v = v.trim();
        if v.eq_ignore_ascii_case("off") {
            return 1.0;
        }
        if let Ok(f) = v.parse::<f32>() {
            return f.max(0.0);
        }
    }
    if LightMath::from_env() != LightMath::Exact {
        return 1.0;
    }
    if lightset.f("keeplook", 0.0) != 0.0 {
        return ColorScales::read(lightset, vision).diffuse.max(1.0);
    }
    lightset.f("h1sunlift", 1.0).max(1.0)
}

/// The sun power (brightest colour channel times the light set's diffuse
/// scale) old-style suns are lifted to when under it: the retail maps'
/// (Shipment 15). Tried 3.8, 8 and 15 against the loading pictures of
/// Rust, Favela and Terminal (5 Nov 2026): 15 came closest, none blew out.
pub const USERMAP_SUN_FLOOR: f32 = 15.0;

/// The floor for an old-style sun under a light set that was already
/// automatic: its author tuned the exposure for the content as it is, and
/// a full lift broke that (Highrise, whose key follows the scene's level,
/// went white; Alpha passed its picture). Favela's level, as before.
pub const AUTO_SUN_FLOOR: f32 = 3.8;

/// Maps whose look the owner approved as it is: never lifted or metered
/// differently. Empty since 9 Nov 2026: Favela (approved 5 Nov) now takes
/// HMW's own vision and script (`hmw_files.ff`) like every map.
pub const KEEP_LOOK: &[&str] = &[];

/// Whether a map keeps its look ([`KEEP_LOOK`]).
pub fn keeps_look(map: &str) -> bool {
    KEEP_LOOK.iter().any(|m| m.eq_ignore_ascii_case(map))
}

/// The range of sun power that marks an old-style (MW2, MW3 and MWR port)
/// sun: their maps were lit for a renderer that showed light values as
/// display values (sun 1 to 6; the retail and AW suns are 8 to 60). Under
/// 0.9 are the AW night and moonlit maps (Highrise2, Lair 0.5), lit dim on
/// purpose. Measured over the 64 Horizon MW usermaps (5 Nov 2026).
pub const OLD_SUN_MIN: f32 = 0.9;
pub const OLD_SUN_MAX: f32 = 7.0;

/// The sun's power: its brightest channel times the light set's diffuse
/// scale.
pub fn sun_power(lightset: &Settings, sun_color: [f32; 3]) -> f32 {
    sun_color.iter().cloned().fold(0.0, f32::max) * lightset.f("diffusecolorscale", 1.0)
}

/// Whether a sun of this power is an old-style one ([`OLD_SUN_MIN`] ..
/// [`OLD_SUN_MAX`]).
pub fn is_old_style_sun(power: f32) -> bool {
    (OLD_SUN_MIN..OLD_SUN_MAX).contains(&power)
}

/// INFERRED factor on an old-style sun's diffuse scale (so on the sun and,
/// through `secondary_scale`, the baked shade) when its power is under
/// [`USERMAP_SUN_FLOOR`]: against the maps' own loading pictures, the lit
/// ground sat far too dark beside a sky and fog authored at the retail
/// scale (Rust: sunlit sand at a fifth of the picture's level). 1 for
/// retail and AW suns, night maps, and suns already above the floor.
/// `BO2MP_H1_SUN_FLOOR=<n>` sets the floor, `0` turns it off.
/// `metered`: the light set's fixed exposure is metered instead
/// ([`meters_old_fixed`]), so the full floor applies; else
/// [`AUTO_SUN_FLOOR`].
pub fn usermap_lift(lightset: &Settings, sun_color: [f32; 3], metered: bool) -> f32 {
    let floor = std::env::var("BO2MP_H1_SUN_FLOOR")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(if metered { USERMAP_SUN_FLOOR } else { AUTO_SUN_FLOOR });
    let power = sun_power(lightset, sun_color);
    if floor <= 0.0 || !is_old_style_sun(power) || power >= floor || LightMath::from_env() != LightMath::Exact {
        1.0
    } else {
        floor / power
    }
}

/// Whether a light set's fixed exposure is metered automatically instead.
/// The exe uses the light set's EV (`tonemapauto` 0), but that is too dark
/// here until the missing scale is found (see [`guesses`]): ON by default,
/// `BO2MP_H1_OLD_FIXED=exe` keeps the EV.
pub fn meters_old_fixed(lightset: &Settings, sun_color: [f32; 3]) -> bool {
    lightset.f("tonemapauto", 1.0) == 0.0
        && is_old_style_sun(sun_power(lightset, sun_color))
        && !std::env::var("BO2MP_H1_OLD_FIXED").is_ok_and(|v| v.trim().eq_ignore_ascii_case("exe"))
}

/// The most the placed models' baked light grid takes of the shade factor
/// ([`secondary_scale`]): the level checked on screen with props (favela,
/// 3.48, and Alpha, 3.8). SUSPECTED cause of washed-out, see-through props
/// (5 Nov 2026): the sun lift of 0d633b8 / a4e5554 raised the shade factor to
/// about 10 on Rust-like maps, a level only the ground was compared at (by
/// the loading pictures); props in shade (crates in a container, a fridge)
/// came out pure white while the sunlit ones and the ground looked right.
pub const MODEL_SHADE_CAP: f32 = 3.8;

/// The factor the models' light grid takes: `shade` itself up to
/// [`MODEL_SHADE_CAP`]. `BO2MP_H1_PROPS_SHADE=full` keeps the whole factor
/// (the behaviour of 0d633b8 to a4e5554), a number sets the cap.
pub fn model_shade(shade: f32) -> f32 {
    let cap = match std::env::var("BO2MP_H1_PROPS_SHADE") {
        Ok(v) if v.trim().eq_ignore_ascii_case("full") => f32::INFINITY,
        Ok(v) => v.trim().parse::<f32>().ok().filter(|c| *c > 0.0).unwrap_or(MODEL_SHADE_CAP),
        Err(_) => MODEL_SHADE_CAP,
    };
    shade.min(cap)
}

/// The sun's `LIGHT_DIFFUSE` and `LIGHT_SPECULAR` colours.
pub fn sun_colors(color: Vec3, scales: ColorScales, math: LightMath) -> (Vec3, Vec3) {
    match math {
        LightMath::Exact => (color * scales.diffuse, color * scales.specular),
        LightMath::Fitted => (color * FITTED_SUN_SCALE, color * FITTED_SUN_SCALE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_scale_follows_the_light_set_and_never_darkens() {
        let none = Settings::default();
        assert_eq!(secondary_scale(&none, &none), 1.0);
        let mut ls = Settings::default();
        ls.values.insert("diffusecolorscale".into(), vec![3.48]);
        // an authored scale is the sun's alone
        assert_eq!(secondary_scale(&ls, &none), 1.0);
        ls.values.insert("h1sunlift".into(), vec![9.68]);
        assert_eq!(secondary_scale(&ls, &none), 9.68);
        ls.values.remove("h1sunlift");
        ls.values.insert("keeplook".into(), vec![1.0]);
        assert_eq!(secondary_scale(&ls, &none), 3.48);
        ls.values.insert("diffusecolorscale".into(), vec![0.5]);
        assert_eq!(secondary_scale(&ls, &none), 1.0);
    }

    #[test]
    fn model_shade_is_capped_not_raised() {
        assert_eq!(model_shade(1.0), 1.0);
        assert_eq!(model_shade(3.48), 3.48);
        assert_eq!(model_shade(10.0), MODEL_SHADE_CAP);
    }

    #[test]
    fn old_style_suns_are_lifted_and_others_not() {
        let ls = Settings::default();
        // terminal (fixed or automatic alike)
        assert!((usermap_lift(&ls, [1.55, 1.53, 1.5], true) - USERMAP_SUN_FLOOR / 1.55).abs() < 1e-4);
        // highrise (already automatic): the gentler floor
        assert!((usermap_lift(&ls, [1.0, 1.0, 1.0], false) - AUTO_SUN_FLOOR).abs() < 1e-4);
        // retail / AW
        assert_eq!(usermap_lift(&ls, [15.0, 14.0, 10.0], true), 1.0);
        // AW moonlight (Highrise2)
        assert_eq!(usermap_lift(&ls, [0.18, 0.24, 0.5], true), 1.0);
        // an old-style sun already above the floor
        let mut f = Settings::default();
        f.values.insert("diffusecolorscale".into(), vec![15.0]);
        assert_eq!(usermap_lift(&f, [1.1, 1.0, 1.0], true), 1.0);
        assert!(!keeps_look("mp_favela") && !keeps_look("mp_rust"));
    }

    #[test]
    fn only_old_style_fixed_exposure_is_metered() {
        let mut fixed = Settings::default();
        fixed.values.insert("tonemapauto".into(), vec![0.0]);
        assert!(meters_old_fixed(&fixed, [1.5, 1.3, 1.0]));
        assert!(!meters_old_fixed(&fixed, [15.0, 14.0, 10.0]));
        assert!(!meters_old_fixed(&Settings::default(), [1.5, 1.3, 1.0]));
    }

    #[test]
    fn exact_sun_is_the_light_unscaled_without_a_light_set() {
        let s = ColorScales::read(&Settings::default(), &Settings::default());
        assert_eq!(
            s,
            ColorScales {
                diffuse: 1.0,
                specular: 1.0
            }
        );
        // Shipment's sun, as its zone holds it.
        let c = Vec3::new(15.0, 14.323892, 10.309335);
        let (d, sp) = sun_colors(c, s, LightMath::Exact);
        assert_eq!((d, sp), (c, c));
        let (fd, _) = sun_colors(c, s, LightMath::Fitted);
        assert!((fd.x / d.x - std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn light_set_scales_and_tweaks_multiply() {
        let mut ls = Settings::default();
        ls.values.insert("diffusecolorscale".into(), vec![1.5]);
        ls.values.insert("specularcolorscale".into(), vec![0.5]);
        let mut v = Settings::default();
        v.values
            .insert("r_primarylighttweakdiffusestrength".into(), vec![2.0]);
        v.values
            .insert("r_primarylighttweakspecularstrength".into(), vec![3.0]);
        // Tweaks off: the light set alone.
        let s = ColorScales::read(&ls, &v);
        assert_eq!((s.diffuse, s.specular), (1.5, 0.5));
        v.values.insert("r_primarylightusetweaks".into(), vec![1.0]);
        let s = ColorScales::read(&ls, &v);
        assert_eq!((s.diffuse, s.specular), (3.0, 1.5));
    }

    #[test]
    fn parse_switch() {
        assert_eq!(LightMath::parse("EXACT"), LightMath::Exact);
        assert_eq!(LightMath::parse("fitted"), LightMath::Fitted);
        assert_eq!(LightMath::parse(""), LightMath::Exact);
    }
}
