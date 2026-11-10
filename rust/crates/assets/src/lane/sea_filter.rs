//! bo2mp: the filtering of Black Ops II's seas (`cod7watershore`: Carrier,
//! Takeoff, Frostbite's distant sea). The game's sea shader reads three
//! scrolling normal maps and draws two sharp sun-glint lobes from them; at
//! the game's own resolution and with its post filter that is a soft sheen.
//! Here a far ripple spans many pixels and the lobes (powers in the
//! hundreds) pick single bright texels: salt-and-pepper glitter, brighter
//! still under the Black Ops III look's sun. The water shader
//! (`geometry_diagnostic.wgsl`, WATER) reads two more rows of the sea's
//! table (25 and 26) that this module writes:
//!
//! * x: specular anti-aliasing (`BO2MP_SEA_AA`, 1): the glint lobe is widened
//!   by the half vector's change across the pixel, energy kept;
//! * y: the share of the live sun shadow that reaches the sea
//!   (`BO2MP_SEA_SHADOW`, 1): the game's own sea term is the baked shadow
//!   plus 0.75, so a ship's or the carrier's shadow can darken the sea by a
//!   quarter at most, never to a hard dark wedge;
//! * z, w, x: the small ripples fade toward flat from `BO2MP_SEA_FAR_START`
//!   (2500 units) to `BO2MP_SEA_FAR_END` (14000) by `BO2MP_SEA_FAR` (0.6);
//! * y of the second row: on. `BO2MP_SEA_FILTER=0` writes it 0 and the sea
//!   draws as before.

fn env_f32(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

pub(super) fn enabled() -> bool {
    codlegacy_env::var("BO2MP_SEA_FILTER").map_or(true, |v| v.trim() != "0")
}

/// Rows 25 and 26 of a sea's water table.
pub(super) fn rows() -> [[f32; 4]; 2] {
    let on = enabled();
    let start = env_f32("BO2MP_SEA_FAR_START", 2500.0);
    let end = env_f32("BO2MP_SEA_FAR_END", 14000.0).max(start + 1.0);
    [
        [
            env_f32("BO2MP_SEA_AA", 1.0),
            env_f32("BO2MP_SEA_SHADOW", 1.0).min(1.0),
            env_f32("BO2MP_SEA_FAR", 0.6).min(1.0),
            start,
        ],
        [end, if on { 1.0 } else { 0.0 }, 0.0, 0.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_default_on_with_a_far_range_that_runs_outward() {
        let [a, b] = rows();
        assert_eq!(b[1], 1.0);
        assert!(a[3] < b[0]);
        assert!(a[2] > 0.0 && a[2] <= 1.0);
    }
}
