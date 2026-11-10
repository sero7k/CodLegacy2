//! bo2mp: Black Ops II's models (the first-person gun and arms, players) lit
//! on a Modern Warfare 2 or 3 map, in the units that map's own models use:
//! the light grid's colour read as `(2 * colour)^2` (`decode_model_lighting_sample`)
//! plus the sun, `light_diffuse` of the sun light's colour, by N.L and how
//! much of the point the sun reaches.

use crate::{R_COLOR_SCALE_DEFAULT, decode_model_lighting_sample, light_diffuse};

/// The grid's light at a point from its sampled tile's mean colour (0..1).
pub fn guest_grid_light(mean: [f32; 3]) -> [f32; 3] {
    decode_model_lighting_sample(mean)
}

/// The sun's colour as the map's own lit models take it (linear). A colour
/// stored brighter than 1 is scaled down to 1 at its brightest channel first,
/// keeping its hue: past 1 the curve rises as x^2.4 and one such sun drew
/// every gun and player white.
pub fn guest_sun_linear(color: [f32; 3]) -> [f32; 3] {
    let peak = color.iter().copied().fold(0.0f32, f32::max);
    let scaled = if peak > 1.0 {
        color.map(|c| c / peak)
    } else {
        color
    };
    let lit = light_diffuse(scaled.map(|c| c.max(0.0)), R_COLOR_SCALE_DEFAULT);
    [lit[0], lit[1], lit[2]]
}

/// Whether the sun lights Black Ops II's models on another game's map:
/// `BO2MP_FOREIGN_SUN` set to `0` or `1` decides; unset, on wherever the
/// map's light grid says where the sun reaches (Modern Warfare 2 and 3,
/// whose grids hold the light without the sun; Call of Duty 4 and World at
/// War, `legacy`). Without it a gun in full sun took only the shade's light.
pub fn guest_sun_on(iw_grid: bool, legacy: bool, setting: Option<&str>) -> bool {
    match setting.map(str::trim) {
        Some("1") => true,
        Some("0") => legacy,
        _ => iw_grid || legacy,
    }
}
