//! The rules of Ghosts' scopes that need no GPU: the ADS field of view, the HUD overlay's alpha
//! and when it draws, when the picture-in-picture lens view runs and how strongly, which reticle
//! model a weapon takes, and which reticle bones show. All of it is read from the exe
//! (`docs/exe_notes/IW6_SCOPES.md`, sections 1, 3 and 5); a mark says how sure: READ is the code
//! as read, INFERRED is a meaning put to a value the code only moves.
//!
//! Not here (the thermal technique shaders are not in the install; hold-breath and scope
//! drift are not decoded): see the notes' unknowns.

/// The base field of view (`cg_fov`), its scale and bounds, and the third-person factor
/// (READ: `cg_fov` 65 (65..80), `cg_fovScale` 1 (0.2..2), `cg_fovMin` 1, top 170).
pub fn base_fov(cg_fov: f32, fov_scale: f32, fov_min: f32, third_person: bool) -> f32 {
    let mut fov = cg_fov * fov_scale;
    if third_person {
        fov *= 0.9;
    }
    fov.clamp(fov_min, 170.0)
}

/// The zoomed field of view of a weapon: variable zoom steps through 25, 15 and 8 degrees by the
/// zoom level index; any other scope takes the weapon's own ADS zoom (the product of the
/// attachments' scales times the attachment's or the weapon's `adsZoomFov`). Never wider than the
/// base. READ.
pub fn ads_zoom_fov(base: f32, variable_zoom: bool, zoom_level: usize, ads_zoom_fov: f32) -> f32 {
    let fov = if variable_zoom { [25.0, 15.0, 8.0][zoom_level.min(2)] } else { ads_zoom_fov };
    fov.min(base)
}

/// The field of view while aiming (not a hybrid scope, no lens view): linear in degrees from the
/// base to the zoomed one by the remapped ADS progress `f`. READ.
pub fn ads_fov(base: f32, zoomed: f32, f: f32) -> f32 {
    (1.0 - f) * base + f * zoomed
}

/// The remapped ADS progress: `p` is the aim progress (0 hip, 1 aimed), `frac` the zoom fraction
/// of the weapon's ADS settings (`zoomInFrac` going in, `zoomOutFrac` coming out; INFERRED which
/// of the two the engine's flag selects), `scale` the product of the attachments' ADS scales (a
/// scale at or under 0 counts as 1). The zoom happens in the last `frac` of the progress. READ.
pub fn ads_fraction(p: f32, frac: f32, scale: f32) -> f32 {
    if p >= 1.0 {
        return 1.0;
    }
    if p <= 0.0 {
        return 0.0;
    }
    let z = frac * if scale <= 0.0 { 1.0 } else { scale };
    if z <= 0.0 {
        return 0.0;
    }
    ((p - (1.0 - z)) / z).clamp(0.0, 1.0)
}

/// A hybrid scope's field of view by its animation state (0..5): 2 holds the main settings' zoom,
/// 3 the other's, 4 and 5 blend between them by `f` (4 from the main to the other, 5 back); 0 and
/// 1 are the plain ADS. READ.
pub fn hybrid_fov(state: u8, base: f32, main: f32, other: f32, f: f32, plain: f32) -> f32 {
    let (a, b) = (main.min(base), other.min(base));
    match state {
        2 => a,
        3 => b,
        4 => a + (b - a) * f,
        5 => b + (a - b) * f,
        _ => plain,
    }
}

/// What a hybrid's animation weights say of its state (READ, `0x1f`, `0x20`, `0x21` the three
/// animations): the second running is state 4 while it plays and 3 after; the third state 5 and 2;
/// the first alone state 1; none state 0.
pub fn hybrid_state(first: bool, second: bool, second_running: bool, third: bool, third_running: bool) -> u8 {
    if second {
        if second_running { 4 } else { 3 }
    } else if third {
        if third_running { 5 } else { 2 }
    } else if first {
        1
    } else {
        0
    }
}

/// Whether a scope picture overlay is on: it has a picture, or a reticle with no lens view
/// running, and the aim has begun (alpha over 0.01). READ.
pub fn overlay_active(has_picture: bool, has_reticle: bool, dual_view: bool, alpha: f32) -> bool {
    (has_picture || (has_reticle && !dual_view)) && alpha > 0.01
}

/// The overlay is drawn only once the aim is complete (0.99) and the weapon is not busy
/// (reload, sprint, mantle: the 0x3800 mask). READ.
pub fn overlay_drawn(p: f32, busy_mask: u32) -> bool {
    p >= 0.99 && busy_mask & 0x3800 == 0
}

/// The overlay's centre on a 640x480 screen: the middle plus the sway, which the engine projects
/// from the aim axis (`dx`, `dy`, `dz` the weapon-aim offset in the view's axes, `tan_half_x` and
/// `tan_half_y` the field's half tangents). READ.
pub fn overlay_centre(dx: f32, dy: f32, dz: f32, tan_half_x: f32, tan_half_y: f32) -> [f32; 2] {
    if dz.abs() < 1e-6 {
        return [320.0, 240.0];
    }
    [320.0 + -320.0 * dx / (tan_half_x * dz), 240.0 + -240.0 * dy / (tan_half_y * dz)]
}

/// The lens view (picture-in-picture) of a scope with `useDualFOV`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lens {
    /// The second scene is drawn.
    pub active: bool,
    /// How far in (0..1) when active.
    pub t: f32,
    /// Fading out below the activation (1 at hip, 0 at the threshold).
    pub fade: f32,
    /// The second view's blur strength (INFERRED a blur or depth-of-field strength).
    pub blur: f32,
    /// The second view's zoom scale over the base field (INFERRED a projection scale).
    pub zoom: f32,
}

/// The lens view by the aim progress `p`, going in or coming out; `was_active` is last frame's.
/// Going in it switches on above 0.4 and ramps over the last 0.6; coming out it stays on until
/// 0.7 and ramps over 0.3; below that it is off and `fade` counts the way down. Requires
/// `useDualFOV` and an idle weapon. READ.
pub fn lens(p: f32, going_in: bool, was_active: bool, dual_fov: bool, busy_mask: u32) -> Lens {
    if !dual_fov || busy_mask & 0x3800 != 0 {
        return Lens { zoom: 1.0, ..Lens::default() };
    }
    let (on, t) = if going_in {
        (p > 0.4, (p - 0.4) / 0.6)
    } else if was_active {
        (p >= 0.7, (p - 0.7) / 0.3)
    } else {
        (false, 0.0)
    };
    if on || (!going_in && was_active && p >= 0.7) {
        let t = t.clamp(0.0, 1.0);
        Lens { active: true, t, fade: 0.0, blur: t * 14.0 / 3.0, zoom: 8.0 * t + 1.0 }
    } else {
        Lens { active: false, t: 0.0, fade: ((0.4 - p) / 0.4).clamp(0.0, 1.0), blur: 0.0, zoom: 1.0 }
    }
}

/// The lens picture's blend ramp: `r_ssrFadeInDuration` (0.05 s, 0.01..1) to full, reset to 0 when
/// the view switches on or off. READ.
pub fn lens_blend(blend: f32, dt: f32, fade_in_secs: f32, toggled: bool) -> f32 {
    if toggled {
        return 0.0;
    }
    (blend + dt / fade_in_secs.clamp(0.01, 1.0)).clamp(0.0, 1.0)
}

/// The reticle index a packed weapon id carries: its top six bits (READ, `>> 26`).
pub fn reticle_index(packed_weapon: u32) -> usize {
    (packed_weapon >> 26) as usize & 0x3f
}

/// Where a reticle model comes from: the sight attachment's own 16 for indices under 16, else
/// one of three extension banks of 16 (`reticlehack`, `reticlehack2`, `reticlehack3`). READ.
/// Returns (bank, slot): bank 0 is the attachment's.
pub fn reticle_slot(index: usize) -> Option<(u8, usize)> {
    (index < 64).then_some(((index / 16) as u8, index % 16))
}

/// Which of a red-dot scope's bones show: `(hamr, on, reflex)`. EMP hides all three; alt mode, or a
/// hybrid-capable scope not in its hybrid state, shows the on and reflex bones; else the hamr
/// bone. READ.
pub fn red_dot_bones(emp: bool, alt_mode: bool, hybrid_capable: bool, hybrid_scope: bool) -> (bool, bool, bool) {
    if emp {
        return (false, false, false);
    }
    if alt_mode || (hybrid_capable && !hybrid_scope) {
        (false, true, true)
    } else {
        (true, false, false)
    }
}

/// The reticle bones an EMP hides on every sight. READ.
pub const EMP_HIDDEN_BONES: [&str; 6] = [
    "tag_reticle_acog",
    "tag_reticle_red_dot",
    "tag_eotech_reticle",
    "tag_magnifier_eotech_reticle",
    "tag_reticle_tavor_scope",
    "tag_reticle_thermal_scope",
];

/// Whether the player may hold breath in this ADS: the weapon (or its scope) says so, or a perk
/// does (the exe names three perks: the plain, the improved and the "while ads" one). INFERRED
/// from the field names; the timing code was not found in the exe.
pub fn can_hold_breath(weapon_flag: bool, perk_holdbreath: bool, perk_while_ads: bool, ads: bool) -> bool {
    ads && (weapon_flag || perk_holdbreath || perk_while_ads)
}

/// The hold-breath meter: `held` seconds spent so far against `limit` (the base seconds plus the
/// perk's extra). Holding fills it, letting go drains it at `recover_rate` times the fill rate.
/// INFERRED shape; the numbers are the caller's.
pub fn hold_breath_step(held: f32, limit: f32, holding: bool, dt: f32, recover_rate: f32) -> (f32, bool) {
    let h = if holding { held + dt } else { held - dt * recover_rate.max(0.0) };
    let h = h.clamp(0.0, limit.max(0.0));
    // Out of breath once the limit is reached; a held breath can start again once it has drained.
    (h, holding && h < limit)
}

/// The breath sound's dvars as the exe registers them: the lerp rate and the delay in seconds.
pub const BREATH_SND_LERP_DEFAULT: f32 = 2.0;
pub const BREATH_SND_DELAY_DEFAULT: f32 = 1.0;

/// The lens material's sample rectangle (`SCOPE_SAMPLE_LIMITS_PS`): the main view's viewport
/// `(x, y, w, h)` in pixels inside a scene target of `target` pixels, as
/// `((x + 0.5) / W, (y + 0.5) / H, (x + w - 0.5) / W, (y + h - 0.5) / H)`: a half-texel inset, so
/// a clamped sample never reads outside the picture. READ (the constant is written only for the
/// picture-in-picture view).
pub fn sample_limits(viewport: [f32; 4], target: [f32; 2]) -> [f32; 4] {
    let [x, y, w, h] = viewport;
    let [tw, th] = [target[0].max(1.0), target[1].max(1.0)];
    [(x + 0.5) / tw, (y + 0.5) / th, (x + w - 0.5) / tw, (y + h - 0.5) / th]
}

/// Where the lens material samples the scene: its pixel shader (READ, decoded from the shader's
/// bytecode; `docs/exe_notes/IW6_SCOPES.md`, "Lens shader") reads a two-channel offset map at the
/// quad's texture coordinates, takes `(offset - 0.5) * scale + base`, and clamps the result to
/// the sample limits (`min` of the high corner after `max` of the low corner). The colour out is
/// that scene sample times the material's colour map.
///
/// `offset` is the offset map's value, `scale` and `base` the two interpolants the vertex stage
/// passes (`scale` from the pixel-to-UV constant, `base` the screen position's UV).
pub fn lens_sample_uv(offset: [f32; 2], scale: [f32; 2], base: [f32; 2], limits: [f32; 4]) -> [f32; 2] {
    std::array::from_fn(|k| ((offset[k] - 0.5) * scale[k] + base[k]).max(limits[k]).min(limits[2 + k]))
}

/// The thermal vision's three pixel constants (READ, exe `THERMAL_*`): colour offset
/// `(offset, scale, detail scale, 0)`; fade control `(mode, 1 / (max - min), -min / (max - min),
/// 0)` and, when `min >= max`, scale 1 and bias `-min`; fade colour as the dvar's 0..255 bytes
/// over 255.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalConstants {
    pub color_offset: [f32; 4],
    pub fade_control: [f32; 4],
    pub fade_color: [f32; 4],
}

/// Defaults of the thermal dvars (READ): colour scale 1 (-4..4), offset 0, detail scale 1, fade
/// control 0 (disabled), fade min 400, max 1000 (0..10000), colour (0, 0, 0, 0).
pub fn thermal_constants(offset: f32, scale: f32, detail: f32, fade_mode: i32, fade_min: f32, fade_max: f32, fade_rgba: [u8; 4]) -> ThermalConstants {
    let (mn, mx) = (fade_min.clamp(0.0, 10000.0), fade_max.clamp(0.0, 10000.0));
    let (k, b) = if mn >= mx { (1.0, -mn) } else { (1.0 / (mx - mn), -mn / (mx - mn)) };
    ThermalConstants {
        color_offset: [offset, scale.clamp(-4.0, 4.0), detail, 0.0],
        fade_control: [fade_mode as f32, k, b, 0.0],
        fade_color: fade_rgba.map(|c| f32::from(c) / 255.0),
    }
}

/// Sets the scope, distortion and thermal engine constants for a frame with no scope view and no
/// thermal vision: the sample limits cover the whole scene (inset by half a texel when the scene
/// size is known, else 0..1), the distortion limits likewise (INFERRED: the same rectangle; the
/// exe writes that constant next to the scope one), thermal at its dvar defaults with the fade off.
/// A pass that asks for them then finds a value instead of a missing constant. `BO2MP_IW6_SCOPE_CONSTS=0`
/// leaves them unset (the old behaviour).
pub fn set_idle_constants(c: &mut crate::code::CodeValues, scene: Option<(u32, u32)>) {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_SCOPE_CONSTS");
    if !S.get() {
        return;
    }
    let lim = match scene {
        Some((w, h)) => sample_limits([0.0, 0.0, w as f32, h as f32], [w as f32, h as f32]),
        None => [0.0, 0.0, 1.0, 1.0],
    };
    c.set_vec("SCOPE_SAMPLE_LIMITS_PS", lim);
    c.set_vec("DISTORTION_SAMPLE_LIMITS_PS", lim);
    let t = thermal_constants(0.0, 1.0, 1.0, 0, 400.0, 1000.0, [0; 4]);
    c.set_vec("THERMAL_COLOR_OFFSET", t.color_offset);
    c.set_vec("THERMAL_FADE_CONTROL", t.fade_control);
    c.set_vec("THERMAL_FADE_COLOR", t.fade_color);
}

/// The thermal blur: while thermal is on and the blur is above 0 it is the larger of the base and
/// the scope's (`thermalBlurFactorScope`) or the full-screen one's (`...NoScope`), both dvars
/// defaulting to 250 (0..10000). READ.
pub fn thermal_blur(base: f32, on: bool, scope_thermal: bool, full_screen: bool, scope_factor: f32, no_scope_factor: f32) -> f32 {
    if !on || base <= 0.0 {
        return base;
    }
    if scope_thermal {
        base.max(scope_factor)
    } else if full_screen {
        base.max(no_scope_factor)
    } else {
        base
    }
}

/// A row of `mp/reticleTable.csv` as far as the exe reads it: column 1 is the reticle's name (the
/// key a lookup matches) and column 6 an integer, the index of the reticle's bit in the player's
/// `unlockedReticles` bank (a value under 64 in a 64-bit bank: INFERRED from the bank test and set
/// that follow the read). The table is not in the install; no other column is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReticleRow {
    pub name: String,
    pub unlock_bit: Option<u32>,
}

/// Reticle rows from a string table's cells (`columns` per row, row-major): rows without a name
/// in column 1 are skipped; an unreadable number reads as none (`atoi` of text is 0 in the engine,
/// which this keeps apart by treating non-numeric as none).
pub fn parse_reticle_table(cells: &[String], columns: usize) -> Vec<ReticleRow> {
    if columns < 7 {
        return Vec::new();
    }
    cells
        .chunks_exact(columns)
        .filter(|r| !r[1].trim().is_empty())
        .map(|r| ReticleRow { name: r[1].trim().to_owned(), unlock_bit: r[6].trim().parse().ok() })
        .collect()
}

/// The unlock bit of the named reticle (the first row matching, as the lookup does), if the row
/// says one and it fits the 64-bit bank.
pub fn reticle_unlock_bit(rows: &[ReticleRow], name: &str) -> Option<u32> {
    rows.iter().find(|r| r.name.eq_ignore_ascii_case(name)).and_then(|r| r.unlock_bit).filter(|b| *b < 64)
}

/// Whether the player's `unlockedReticles` bank (bit set) holds the reticle.
pub fn reticle_unlocked(bank: u64, bit: u32) -> bool {
    bit < 64 && bank >> bit & 1 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breath_can_be_held_only_in_ads_with_a_weapon_or_perk_that_allows_it() {
        assert!(can_hold_breath(true, false, false, true));
        assert!(can_hold_breath(false, true, false, true));
        assert!(!can_hold_breath(true, true, true, false), "not outside ADS");
        assert!(!can_hold_breath(false, false, false, true));
    }

    #[test]
    fn the_breath_runs_out_and_comes_back() {
        let (mut h, mut ok) = (0.0, true);
        for _ in 0..10 {
            (h, ok) = hold_breath_step(h, 4.0, true, 0.5, 1.0);
        }
        assert_eq!(h, 4.0);
        assert!(!ok, "out of breath at the limit");
        (h, ok) = hold_breath_step(h, 4.0, false, 1.0, 0.5);
        assert_eq!((h, ok), (3.5, false));
        assert_eq!(hold_breath_step(0.0, 4.0, false, 1.0, 1.0).0, 0.0, "never below 0");
        assert!(hold_breath_step(1.0, 4.0, true, 0.5, 1.0).1);
    }

    #[test]
    fn the_zoom_happens_in_the_last_of_the_aim() {
        assert_eq!(ads_fraction(0.0, 0.5, 1.0), 0.0);
        assert_eq!(ads_fraction(1.0, 0.5, 1.0), 1.0);
        assert_eq!(ads_fraction(0.5, 0.5, 1.0), 0.0);
        assert!((ads_fraction(0.75, 0.5, 1.0) - 0.5).abs() < 1e-6);
        // A scale of 0 or less counts as 1; a scale of 2 doubles the share.
        assert_eq!(ads_fraction(0.75, 0.5, 0.0), ads_fraction(0.75, 0.5, 1.0));
        assert!((ads_fraction(0.75, 0.25, 2.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_field_of_view_runs_from_the_base_to_the_zoom() {
        let base = base_fov(65.0, 1.0, 1.0, false);
        assert_eq!(base, 65.0);
        assert!((base_fov(65.0, 1.0, 1.0, true) - 58.5).abs() < 1e-4);
        assert_eq!(base_fov(80.0, 2.0, 1.0, false), 160.0);
        assert_eq!(base_fov(80.0, 2.5, 1.0, false), 170.0);
        let z = ads_zoom_fov(base, false, 0, 30.0);
        assert_eq!(ads_fov(base, z, 0.0), 65.0);
        assert_eq!(ads_fov(base, z, 1.0), 30.0);
        assert!((ads_fov(base, z, 0.5) - 47.5).abs() < 1e-5);
        // A zoom wider than the base is held to the base; variable zoom steps 25, 15, 8.
        assert_eq!(ads_zoom_fov(base, false, 0, 90.0), 65.0);
        assert_eq!([0, 1, 2, 9].map(|i| ads_zoom_fov(base, true, i, 90.0)), [25.0, 15.0, 8.0, 8.0]);
    }

    #[test]
    fn a_hybrid_scope_moves_between_its_two_zooms() {
        assert_eq!(hybrid_fov(2, 65.0, 20.0, 40.0, 0.3, 50.0), 20.0);
        assert_eq!(hybrid_fov(3, 65.0, 20.0, 40.0, 0.3, 50.0), 40.0);
        assert!((hybrid_fov(4, 65.0, 20.0, 40.0, 0.25, 50.0) - 25.0).abs() < 1e-5);
        assert!((hybrid_fov(5, 65.0, 20.0, 40.0, 0.25, 50.0) - 35.0).abs() < 1e-5);
        assert_eq!(hybrid_fov(1, 65.0, 20.0, 40.0, 0.25, 50.0), 50.0);
        assert_eq!(hybrid_state(true, false, false, false, false), 1);
        assert_eq!(hybrid_state(true, true, true, false, false), 4);
        assert_eq!(hybrid_state(true, true, false, false, false), 3);
        assert_eq!(hybrid_state(true, false, false, true, true), 5);
        assert_eq!(hybrid_state(true, false, false, true, false), 2);
    }

    #[test]
    fn the_overlay_draws_only_when_the_aim_is_complete() {
        assert!(overlay_active(true, false, false, 0.5));
        assert!(!overlay_active(true, false, false, 0.005));
        assert!(!overlay_active(false, true, true, 1.0), "a reticle gives way to the lens view");
        assert!(overlay_active(false, true, false, 1.0));
        assert!(overlay_drawn(0.99, 0) && !overlay_drawn(0.98, 0) && !overlay_drawn(1.0, 0x800));
        assert_eq!(overlay_centre(0.0, 0.0, 10.0, 0.7, 0.5), [320.0, 240.0]);
        let c = overlay_centre(1.0, -1.0, 10.0, 0.5, 0.5);
        assert!(c[0] < 320.0 && c[1] > 240.0);
    }

    #[test]
    fn the_lens_view_switches_on_going_in_and_stays_longer_coming_out() {
        let off = lens(0.3, true, false, true, 0);
        assert!(!off.active && (off.fade - 0.25).abs() < 1e-6);
        let on = lens(0.7, true, false, true, 0);
        assert!(on.active && (on.t - 0.5).abs() < 1e-6);
        assert!((on.blur - 0.5 * 14.0 / 3.0).abs() < 1e-5 && (on.zoom - 5.0).abs() < 1e-5);
        let full = lens(1.0, true, true, true, 0);
        assert!((full.zoom - 9.0).abs() < 1e-5 && (full.blur - 14.0 / 3.0).abs() < 1e-5);
        // Coming out it stays on down to 0.7.
        assert!(lens(0.75, false, true, true, 0).active);
        assert!(!lens(0.69, false, true, true, 0).active);
        assert!(!lens(0.9, false, false, true, 0).active);
        // Without the dual field of view, or with the weapon busy, never.
        assert!(!lens(1.0, true, true, false, 0).active);
        assert!(!lens(1.0, true, true, true, 0x1000).active);
    }

    #[test]
    fn the_lens_picture_fades_in_over_a_twentieth_of_a_second() {
        let mut b = 0.0;
        for _ in 0..3 {
            b = lens_blend(b, 0.01, 0.05, false);
        }
        assert!((b - 0.6).abs() < 1e-5);
        assert_eq!(lens_blend(b, 0.01, 0.05, true), 0.0);
        assert_eq!(lens_blend(0.9, 1.0, 0.05, false), 1.0);
    }

    #[test]
    fn a_reticle_is_picked_by_the_top_bits_of_the_weapon_id() {
        let packed = 5u32 | (37 << 26);
        assert_eq!(reticle_index(packed), 37);
        assert_eq!(reticle_slot(3), Some((0, 3)));
        assert_eq!(reticle_slot(16), Some((1, 0)));
        assert_eq!(reticle_slot(37), Some((2, 5)));
        assert_eq!(reticle_slot(63), Some((3, 15)));
        assert_eq!(reticle_slot(64), None);
    }

    #[test]
    fn the_red_dot_shows_the_bones_of_its_state() {
        assert_eq!(red_dot_bones(true, true, true, true), (false, false, false));
        assert_eq!(red_dot_bones(false, true, false, false), (false, true, true));
        assert_eq!(red_dot_bones(false, false, true, false), (false, true, true));
        assert_eq!(red_dot_bones(false, false, true, true), (true, false, false));
        assert_eq!(red_dot_bones(false, false, false, false), (true, false, false));
    }

    #[test]
    fn the_sample_limits_inset_the_viewport_by_half_a_texel() {
        let l = sample_limits([0.0, 0.0, 512.0, 512.0], [512.0, 512.0]);
        assert_eq!(l, [0.5 / 512.0, 0.5 / 512.0, 511.5 / 512.0, 511.5 / 512.0]);
        let l = sample_limits([100.0, 50.0, 200.0, 100.0], [400.0, 200.0]);
        assert!((l[0] - 100.5 / 400.0).abs() < 1e-6 && (l[3] - 149.5 / 200.0).abs() < 1e-6);
    }

    #[test]
    fn the_lens_samples_around_its_base_and_never_leaves_the_limits() {
        let lim = [0.1, 0.1, 0.9, 0.9];
        // A neutral offset (0.5) samples straight at the base.
        assert_eq!(lens_sample_uv([0.5, 0.5], [0.2, 0.2], [0.4, 0.6], lim), [0.4, 0.6]);
        // An offset pushes by (offset - 0.5) * scale.
        let uv = lens_sample_uv([1.0, 0.0], [0.2, 0.2], [0.4, 0.6], lim);
        assert!((uv[0] - 0.5).abs() < 1e-6 && (uv[1] - 0.5).abs() < 1e-6);
        // Past the rectangle it holds at its edge.
        assert_eq!(lens_sample_uv([1.0, 0.0], [4.0, 4.0], [0.5, 0.5], lim), [0.9, 0.1]);
    }

    #[test]
    fn the_thermal_constants_follow_the_dvars() {
        let c = thermal_constants(0.0, 1.0, 1.0, 0, 400.0, 1000.0, [0, 0, 0, 0]);
        assert_eq!(c.color_offset, [0.0, 1.0, 1.0, 0.0]);
        assert!((c.fade_control[1] - 1.0 / 600.0).abs() < 1e-9 && (c.fade_control[2] + 400.0 / 600.0).abs() < 1e-6);
        // A reversed range: scale 1, bias -min.
        let c = thermal_constants(0.5, 9.0, 2.0, 1, 800.0, 200.0, [255, 51, 0, 255]);
        assert_eq!(c.color_offset, [0.5, 4.0, 2.0, 0.0]);
        assert_eq!(c.fade_control, [1.0, 1.0, -800.0, 0.0]);
        assert_eq!(c.fade_color, [1.0, 0.2, 0.0, 1.0]);
    }

    #[test]
    fn thermal_blur_takes_the_larger_of_the_base_and_the_mode_factor() {
        assert_eq!(thermal_blur(0.0, true, true, false, 250.0, 250.0), 0.0);
        assert_eq!(thermal_blur(10.0, false, true, false, 250.0, 250.0), 10.0);
        assert_eq!(thermal_blur(10.0, true, true, false, 250.0, 100.0), 250.0);
        assert_eq!(thermal_blur(10.0, true, false, true, 250.0, 100.0), 100.0);
        assert_eq!(thermal_blur(300.0, true, true, false, 250.0, 100.0), 300.0);
    }

    #[test]
    fn the_reticle_table_gives_a_bit_by_name() {
        let cells: Vec<String> = ["0", "reticle_dot", "a", "b", "c", "d", "3", "x", "1", "reticle_ring", "a", "b", "c", "d", "70", "x", "2", "", "", "", "", "", "9", "x"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let rows = parse_reticle_table(&cells, 8);
        assert_eq!(rows.len(), 2);
        assert_eq!(reticle_unlock_bit(&rows, "RETICLE_DOT"), Some(3));
        assert_eq!(reticle_unlock_bit(&rows, "reticle_ring"), None, "70 does not fit a 64-bit bank");
        assert_eq!(reticle_unlock_bit(&rows, "nope"), None);
        assert!(parse_reticle_table(&cells, 3).is_empty());
        assert!(reticle_unlocked(0b1000, 3) && !reticle_unlocked(0b1000, 2) && !reticle_unlocked(!0, 64));
    }

    #[test]
    fn the_idle_constants_are_set_for_the_passes_that_read_them() {
        let mut c = crate::code::CodeValues::default();
        set_idle_constants(&mut c, Some((100, 50)));
        assert_eq!(c.get("SCOPE_SAMPLE_LIMITS_PS").unwrap()[0], [0.005, 0.01, 0.995, 0.99]);
        assert_eq!(c.get("DISTORTION_SAMPLE_LIMITS_PS").unwrap()[0], c.get("SCOPE_SAMPLE_LIMITS_PS").unwrap()[0]);
        assert_eq!(c.get("THERMAL_FADE_CONTROL").unwrap()[0][0], 0.0);
        let mut d = crate::code::CodeValues::default();
        set_idle_constants(&mut d, None);
        assert_eq!(d.get("SCOPE_SAMPLE_LIMITS_PS").unwrap()[0], [0.0, 0.0, 1.0, 1.0]);
    }
}
