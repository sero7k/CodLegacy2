//! bo2mp: effect lights held to the map's light units.
//!
//! An effect light's colour comes out in its own game's units: IW games'
//! (`elem_light_color_bgr`: byte / 255 times the element's scale, about 1
//! for a lit surface's sun) or Black Ops II's (`R_AddOmniLightToScene`: the
//! colour linear times alpha times 32, its sun about 15 before exposure).
//! A guest gun's muzzle flash played on another game's map handed its light
//! over unconverted: a Black Ops II flash on a Modern Warfare 2 / 3 map (IW4
//! light pipeline, no exposure) is some fifteen times too bright, the whole
//! view washed white for a frame. Lights are converted here, then held under
//! a peak and radius (a muzzle flash's much lower), and a muzzle flash's
//! spot light (a hard-edged cone along the barrel) is dropped: no gun's
//! flash is a flashlight.

/// The units an effect light's colour is in, or a map's lighting expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxLightUnits {
    /// IW games: about 1 is the sun on a lit surface.
    Iw,
    /// Black Ops II / III: linear HDR before exposure (sun about 15).
    T6Hdr,
}

/// Black Ops II's light units to IW's: the ratio of the two games' suns.
pub const FX_LIGHT_T6_TO_IW: f32 = 1.0 / 15.0;

/// Brightest channel any effect light may carry, in IW units.
pub const FX_LIGHT_PEAK_MAX_IW: f32 = 16.0;

/// Largest radius any effect light may carry (world units).
pub const FX_LIGHT_RADIUS_MAX: f32 = 2048.0;

/// Brightest channel a muzzle flash's light may carry, in IW units (three
/// suns: a flash, never a flashbang).
pub const FX_MUZZLE_LIGHT_PEAK_MAX_IW: f32 = 3.0;

/// Largest radius a muzzle flash's light may carry (world units).
pub const FX_MUZZLE_LIGHT_RADIUS_MAX: f32 = 400.0;

/// What a clamp changed (for the once-per-run log).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxLightClampKind {
    /// Converted between the two unit systems.
    Units,
    /// Brightest channel held under the cap.
    Peak,
    /// Radius held under the cap.
    Radius,
}

impl FxLightClampKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Units => "units",
            Self::Peak => "peak",
            Self::Radius => "radius",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxLightClampNote {
    pub kind: FxLightClampKind,
    pub before: f32,
    pub after: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxLightNormalised {
    pub color_bgr: [f32; 3],
    pub radius: f32,
    /// The strongest change made (peak over radius over units), if any.
    pub note: Option<FxLightClampNote>,
}

#[inline]
fn units_scale(from: FxLightUnits, to: FxLightUnits) -> f32 {
    match (from, to) {
        (FxLightUnits::T6Hdr, FxLightUnits::Iw) => FX_LIGHT_T6_TO_IW,
        // An IW light on a Black Ops II map keeps its colour (as before:
        // dimmer than the map's own, never brighter).
        _ => 1.0,
    }
}

#[inline]
fn peak_cap(to: FxLightUnits, muzzle: bool) -> f32 {
    let iw = if muzzle {
        FX_MUZZLE_LIGHT_PEAK_MAX_IW
    } else {
        FX_LIGHT_PEAK_MAX_IW
    };
    match to {
        FxLightUnits::Iw => iw,
        FxLightUnits::T6Hdr => iw / FX_LIGHT_T6_TO_IW,
    }
}

/// A muzzle flash effect, by its name (`muzzleflashes/...`, Black Ops II's
/// `weapon/muzzleflashes/fx_muz_*`, AW / MWR's `vfx/muzzleflash/...`).
pub fn fx_is_muzzle_flash_name(def_name: &str) -> bool {
    ["muzzle", "muzflash", "fx_muz_", "_flash_view", "_flash_wv", "flash_view_", "flash_wv_"]
        .iter()
        .any(|marker| contains_ignore_case(def_name, marker))
}

/// `hay` contains the lower-case `needle`, ignoring ASCII case.
fn contains_ignore_case(hay: &str, needle: &str) -> bool {
    let (hay, needle) = (hay.as_bytes(), needle.as_bytes());
    needle.is_empty()
        || hay
            .windows(needle.len())
            .any(|w| w.iter().zip(needle).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

/// Whether a light element may add its spot light: never a muzzle flash's.
#[inline]
pub const fn fx_light_spot_allowed(is_spot: bool, muzzle: bool) -> bool {
    !(is_spot && muzzle)
}

/// The light in the map's units, held under the caps; `None` for a light
/// that adds nothing or is not a number.
pub fn fx_light_normalise(
    color_bgr: [f32; 3],
    radius: f32,
    from: FxLightUnits,
    to: FxLightUnits,
    muzzle: bool,
) -> Option<FxLightNormalised> {
    if !radius.is_finite() || radius <= 0.0 || color_bgr.iter().any(|c| !c.is_finite()) {
        return None;
    }
    let mut note = None;
    let mut color = color_bgr.map(|c| c.max(0.0));
    let scale = units_scale(from, to);
    let peak_in = color.iter().copied().fold(0.0f32, f32::max);
    if scale != 1.0 {
        color = color.map(|c| c * scale);
        if peak_in > 0.0 {
            note = Some(FxLightClampNote {
                kind: FxLightClampKind::Units,
                before: peak_in,
                after: peak_in * scale,
            });
        }
    }
    let radius_cap = if muzzle {
        FX_MUZZLE_LIGHT_RADIUS_MAX
    } else {
        FX_LIGHT_RADIUS_MAX
    };
    let radius_out = radius.min(radius_cap);
    if radius_out < radius {
        note = Some(FxLightClampNote {
            kind: FxLightClampKind::Radius,
            before: radius,
            after: radius_out,
        });
    }
    let cap = peak_cap(to, muzzle);
    let peak = color.iter().copied().fold(0.0f32, f32::max);
    if peak > cap {
        // Held under the cap keeping its hue.
        let k = cap / peak;
        color = color.map(|c| c * k);
        note = Some(FxLightClampNote {
            kind: FxLightClampKind::Peak,
            before: peak,
            after: cap,
        });
    }
    if color.iter().all(|c| *c <= 0.0) {
        return None;
    }
    Some(FxLightNormalised {
        color_bgr: color,
        radius: radius_out,
        note,
    })
}

/// Largest screen size of a first-person muzzle flash sprite of Modern
/// Warfare Remastered / Horizon MW (`H1` effects): its half size held under
/// this times its distance from the eye (a half angle of about 14 degrees).
/// OUR CHOICE, from `h1_flash_frame`: the HMW M4's `flash_smoke_fv_child`
/// draws `fx_muzzle_flash_round_01_add_atlas_z40_tip` 25.4 inches (half) at
/// 34 inches from the eye (a 37 degree half angle, two thirds of the view
/// washed orange: the "red mist"); its sibling of the same material is 5.9 at
/// 24 (0.25), the flash's own fireball 2.2 at 24. A flash seen from beyond a
/// few feet is never held.
pub const FX_VIEW_FLASH_SPRITE_TAN_MAX: f32 = 0.25;

/// A flash sprite's half size held to `FX_VIEW_FLASH_SPRITE_TAN_MAX` of its
/// distance from the eye: the factor to scale both sizes by (1 when it
/// already fits, or the distance is not a number).
pub fn fx_view_flash_sprite_scale(size0: f32, eye_dist: f32) -> f32 {
    if !(size0.is_finite() && size0 > 0.0 && eye_dist.is_finite() && eye_dist > 0.0) {
        return 1.0;
    }
    let cap = eye_dist * FX_VIEW_FLASH_SPRITE_TAN_MAX;
    if size0 > cap { cap / size0 } else { 1.0 }
}

/// A camera-lens overlay sprite of a Modern Warfare Remastered / Horizon MW
/// gun flash (`fx_flare_dirty_lens_01_add_nodepth`,
/// `fx_flare_line_cool_squares_iw_add_z0_fog0_nodepth`, `fx_dustonlens_*`):
/// a picture meant to cover the whole screen (37.5 and 75 inches half size
/// 22 inches from the eye on the HMW MORS), lit for a frame or two. Its
/// picture is bright to its edges, so `fx_view_flash_sprite_scale` held to a
/// part of the screen draws it as a hard red-orange square (the "red mist"
/// that stayed after the stand-in fix, 1101678). Black Ops II's own flashes
/// carry no lens overlay, so a gun flash's are not drawn.
pub fn fx_is_lens_overlay_material(material: &str) -> bool {
    ["dirty_lens", "dustonlens", "lensflare", "flare_line", "lens_flare", "lens_dirt"]
        .iter()
        .any(|marker| contains_ignore_case(material, marker))
}

/// A laser sight's model (an attachment or a gun with one built in): the
/// laser's beam (`gfx_laser_light`) is drawn from `tag_laser` only on such
/// a gun. Many guest guns carry `tag_laser` on the bare gun (the mount
/// point for the attachment); a beam from it was a red wedge out of every
/// such gun.
pub fn fx_is_laser_sight_model(model_name: &str) -> bool {
    ["laser", "_lsr", "steadyaim", "_lzr"]
        .iter()
        .any(|marker| contains_ignore_case(model_name, marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mwr_lens_overlays_are_known_and_flash_sprites_are_not() {
        // The HMW MORS flash's two lens sprites (the red square), the
        // Boomhilda's lens dust and line flare.
        for m in [
            "fx_flare_dirty_lens_01_add_nodepth",
            "fx_flare_line_cool_squares_iw_add_z0_fog0_nodepth",
            "fx_flare_line_desat_squares_iw_add_fog0",
            "fx_dustonlens_01_add_slf",
        ] {
            assert!(fx_is_lens_overlay_material(m), "{m}");
        }
        // The flash itself, its glow, smoke and sparks still draw.
        for m in [
            "fx_muzzle_flash_round_01_add_atlas_z40_tip",
            "fx_muzzle_flash_glow_01_add_atlas_z40_tip",
            "fx_glow_quadratic_falloff_add_z5",
            "fx_smk_white_muzzle_blend_atlas_z40_tip",
            "fx_spark_burst_add_atlas",
            "fx_flare_laser_add_z2",
            "gfx_explosion_flash_atlas",
        ] {
            assert!(!fx_is_lens_overlay_material(m), "{m}");
        }
    }

    #[test]
    fn t6_flash_on_iw_map_is_scaled_and_capped() {
        // Black Ops II flash: white, alpha 1 -> 32 in its units.
        let n = fx_light_normalise([32.0; 3], 300.0, FxLightUnits::T6Hdr, FxLightUnits::Iw, true)
            .unwrap();
        let peak = n.color_bgr.iter().copied().fold(0.0f32, f32::max);
        assert!(peak <= FX_MUZZLE_LIGHT_PEAK_MAX_IW + 1e-4, "{peak}");
        assert!((peak - 32.0 / 15.0).abs() < 1e-3, "{peak}");
        assert_eq!(n.note.unwrap().kind, FxLightClampKind::Units);
    }

    #[test]
    fn t6_flash_on_t6_map_untouched() {
        let n = fx_light_normalise(
            [32.0, 20.0, 8.0],
            250.0,
            FxLightUnits::T6Hdr,
            FxLightUnits::T6Hdr,
            true,
        )
        .unwrap();
        assert_eq!(n.color_bgr, [32.0, 20.0, 8.0]);
        assert_eq!(n.radius, 250.0);
        assert_eq!(n.note, None);
    }

    #[test]
    fn huge_guest_light_peak_capped_keeping_hue() {
        // An AW / MWR light whose scale came through raw (hundreds).
        let n = fx_light_normalise([0.0, 200.0, 400.0], 200.0, FxLightUnits::Iw, FxLightUnits::Iw, true)
            .unwrap();
        assert!((n.color_bgr[2] - FX_MUZZLE_LIGHT_PEAK_MAX_IW).abs() < 1e-4);
        assert!((n.color_bgr[1] - FX_MUZZLE_LIGHT_PEAK_MAX_IW / 2.0).abs() < 1e-4);
        let note = n.note.unwrap();
        assert_eq!(note.kind, FxLightClampKind::Peak);
        assert_eq!(note.before, 400.0);
    }

    #[test]
    fn muzzle_radius_capped_other_lights_keep_more() {
        let m = fx_light_normalise([1.0; 3], 5000.0, FxLightUnits::Iw, FxLightUnits::Iw, true).unwrap();
        assert_eq!(m.radius, FX_MUZZLE_LIGHT_RADIUS_MAX);
        let o = fx_light_normalise([1.0; 3], 1000.0, FxLightUnits::Iw, FxLightUnits::Iw, false).unwrap();
        assert_eq!(o.radius, 1000.0);
        assert_eq!(o.note, None);
    }

    #[test]
    fn bad_lights_dropped() {
        assert!(fx_light_normalise([f32::NAN, 1.0, 1.0], 100.0, FxLightUnits::Iw, FxLightUnits::Iw, false).is_none());
        assert!(fx_light_normalise([1.0; 3], 0.0, FxLightUnits::Iw, FxLightUnits::Iw, false).is_none());
        assert!(fx_light_normalise([0.0; 3], 100.0, FxLightUnits::Iw, FxLightUnits::Iw, false).is_none());
    }

    #[test]
    fn muzzle_names_and_spots() {
        assert!(fx_is_muzzle_flash_name("weapon/muzzleflashes/fx_muz_ar_hip_flash"));
        assert!(fx_is_muzzle_flash_name("MuzzleFlashes/AK47_flash_view"));
        assert!(fx_is_muzzle_flash_name("vfx/muzzleflash/h1_ar_flash_view"));
        assert!(!fx_is_muzzle_flash_name("maps/mp_dome/fx_fire_barrel"));
        assert!(!fx_light_spot_allowed(true, true));
        assert!(fx_light_spot_allowed(true, false));
        assert!(fx_light_spot_allowed(false, true));
    }

    #[test]
    fn view_flash_sprite_held_on_screen() {
        // The HMW M4's 25.4 inch glow at 34 inches: held to 8.5.
        let k = fx_view_flash_sprite_scale(25.4, 34.0);
        assert!((25.4 * k - 8.5).abs() < 1e-3);
        // Its 5.9 sibling at 24 and the 2.2 fireball fit already.
        assert_eq!(fx_view_flash_sprite_scale(5.9, 24.0), 1.0);
        assert_eq!(fx_view_flash_sprite_scale(2.2, 24.0), 1.0);
        // Another player's flash across the room is untouched.
        assert_eq!(fx_view_flash_sprite_scale(25.4, 400.0), 1.0);
        assert_eq!(fx_view_flash_sprite_scale(25.4, 0.0), 1.0);
        assert_eq!(fx_view_flash_sprite_scale(f32::NAN, 30.0), 1.0);
    }

    #[test]
    fn laser_models() {
        assert!(fx_is_laser_sight_model("t6_attach_lasersight_view"));
        assert!(fx_is_laser_sight_model("viewmodel_steadyaim"));
        assert!(!fx_is_laser_sight_model("h1_vm_ar_ak47"));
        assert!(!fx_is_laser_sight_model("viewmodel_ak47_mp"));
    }
}
