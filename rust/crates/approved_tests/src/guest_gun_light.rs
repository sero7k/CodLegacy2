//! Black Ops II's guns and players on Modern Warfare 2 and 3 maps: the light
//! they take there and how a Modern Warfare 3 gun's material draws.

use asset_core::T6Blend;
use lighting_iw4::{guest_grid_light, guest_sun_linear, guest_sun_on};
use render_material::catalog::{foreign_model_draw, solid_by_technique_name};

/// What a mid-grey texel (0.5) shows under `light` (the gun shader's
/// `sqrt(texel^2 * light)` at exposure 1, by luminance).
fn grey(light: [f32; 3]) -> f32 {
    (0.25 * (0.2126 * light[0] + 0.7152 * light[1] + 0.0722 * light[2])).sqrt()
}

#[test]
fn grid_colour_reads_as_the_maps_own_models_read_it() {
    assert_eq!(guest_grid_light([0.0; 3]), [0.0; 3]);
    assert_eq!(guest_grid_light([0.5; 3]), [1.0; 3]);
    let l = guest_grid_light([0.25, 0.2, 0.1]);
    assert!((l[0] - 0.25).abs() < 1e-6 && (l[1] - 0.16).abs() < 1e-6 && (l[2] - 0.04).abs() < 1e-6);
}

#[test]
fn sun_colour_is_linear_and_never_past_one() {
    // MW2 Highrise's sun.
    let s = guest_sun_linear([1.0, 0.92, 0.74]);
    assert!(
        (s[0] - 1.0).abs() < 1e-3 && (s[1] - 0.8276).abs() < 2e-3 && (s[2] - 0.5071).abs() < 2e-3
    );
    // A sun stored past 1 keeps its hue at 1, not 3^2.4 (white).
    let hot = guest_sun_linear([3.0, 2.76, 2.22]);
    assert!(hot.iter().all(|c| *c <= 1.0 + 1e-6));
    assert!((hot[1] - s[1]).abs() < 2e-3);
    assert_eq!(guest_sun_linear([0.0; 3]), [0.0; 3]);
    assert_eq!(guest_sun_linear([-1.0, 0.0, 0.0]), [0.0; 3]);
}

#[test]
fn sun_term_on_where_the_grid_leaves_the_sun_out() {
    // MW2 / MW3 grid: on by default.
    assert!(guest_sun_on(true, false, None));
    assert!(!guest_sun_on(true, false, Some("0")));
    // Black Ops grids and grid-less maps: off unless asked.
    assert!(!guest_sun_on(false, false, None));
    assert!(guest_sun_on(false, false, Some("1")));
    // CoD4 / World at War: always.
    assert!(guest_sun_on(false, true, Some("0")));
}

#[test]
fn a_gun_in_mw2_sun_is_lit_like_the_maps_own_models() {
    // Measured on MW2 Highrise (gunlight probe, spawn at eye height in the
    // open): grid tile mean 0.235/0.216/0.184, sun row 1 named by every corner.
    let grid = guest_grid_light([0.235, 0.216, 0.184]);
    let sun = guest_sun_linear([1.0, 0.92, 0.74]);
    let shade_only = grey(grid);
    let lit: [f32; 3] = core::array::from_fn(|c| grid[c] + sun[c] * 0.6);
    // Without the sun a gun in the open drew near black (0.21 of white);
    // with it, as a Modern Warfare 2 prop there: about 0.42.
    assert!(shade_only < 0.25, "{shade_only}");
    assert!(grey(lit) > 0.4 && grey(lit) < 0.5, "{}", grey(lit));
}

#[test]
fn a_modern_warfare_3_gun_with_an_added_light_pass_is_solid() {
    let add = [(2 | (2 << 4) | (1 << 8) | (1 << 27)) as u32, 0];
    let mut d = foreign_model_draw("mc_l_sm_r0c0n0s0p0", 4, &[add], false).unwrap();
    assert_eq!(d.blend, T6Blend::Add);
    solid_by_technique_name("mc_l_sm_r0c0n0s0p0", &mut d);
    assert_eq!(d.blend, T6Blend::Opaque);
    // Glass and blended parts stay blended; sights keep their glow.
    let mut b = foreign_model_draw("mc_l_sm_b0c0n0s0p0", 9, &[add], false).unwrap();
    solid_by_technique_name("mc_l_sm_b0c0n0s0p0", &mut b);
    assert_ne!(b.blend, T6Blend::Opaque);
    let mut r = foreign_model_draw("mc_reflexsight", 9, &[add], false).unwrap();
    solid_by_technique_name("mc_reflexsight", &mut r);
    assert_eq!(r.blend, T6Blend::Add);
}
