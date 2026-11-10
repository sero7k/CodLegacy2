//! Ghosts' world shading maths, as its lit pixel shaders compute it (the
//! DXBC programs of the technique sets in `techsets_*.ff`, read with
//! `dxbc_sm5`; the register maps and the pseudo-code are in
//! `docs/exe_notes/IW6_SHADERS.md`). Pure functions on plain numbers: the
//! lane and the renderers decide where each one is used.
//!
//! Conventions: colours are linear; `v` (view) points from the eye to the
//! surface (positions are camera-relative); `l` is the unit direction
//! towards the light (`LIGHT_POSITION.xyz` of a directional light).

/// The baked light's dominant direction in tangent space, from the two
/// stacked secondary pages' alphas (upper page: x, lower page: y; float
/// texels, -1..1): z is the rest of the unit length.
pub fn lightmap_direction(upper_alpha: f32, lower_alpha: f32) -> [f32; 3] {
    let (x, y) = (upper_alpha, lower_alpha);
    [x, y, (1.0 - x * x - y * y).max(0.0).sqrt()]
}

/// The tangent-space normal the lightmap is lit with, from a normal map's
/// two signed channels (BC5 SNORM, red and green as sampled): the shaders
/// take (2 green, 2 red) as the tangent and binormal slopes and normalise
/// `(x, y, 1)`. `None` (no normal map) is the flat `(0, 0, 1)`.
pub fn tangent_normal(red_green: Option<[f32; 2]>) -> [f32; 3] {
    let Some([r, g]) = red_green else {
        return [0.0, 0.0, 1.0];
    };
    let (x, y) = (2.0 * g, 2.0 * r);
    let inv = 1.0 / (1.0 + x * x + y * y).sqrt();
    [x * inv, y * inv, inv]
}

/// The baked light of a lightmapped surface: the upper page's colour times
/// the tangent normal's z, plus the lower page's colour times the normal's
/// clamped cosine to the baked direction.
pub fn lightmap_light(upper: [f32; 4], lower: [f32; 4], n_ts: [f32; 3]) -> [f32; 3] {
    let d = lightmap_direction(upper[3], lower[3]);
    let f = (n_ts[0] * d[0] + n_ts[1] * d[1] + n_ts[2] * d[2]).clamp(0.0, 1.0);
    std::array::from_fn(|c| upper[c] * n_ts[2] + lower[c] * f)
}

/// On a flat surface (no normal map) the lower page's weight is the baked
/// direction's z. `lightmap_light` with `tangent_normal(None)`.
pub fn lightmap_flat_weight(upper_alpha: f32, lower_alpha: f32) -> f32 {
    lightmap_direction(upper_alpha, lower_alpha)[2]
}

/// The specular strength of the reflection and of the lights' highlights:
/// `envMapParms` (x at normal incidence, y at grazing, z the exponent) over
/// `(1 - |N.V|)^z`.
pub fn env_fresnel(env_map_parms: [f32; 4], n_dot_v: f32) -> f32 {
    let t = (1.0 - n_dot_v.abs()).max(0.0).powf(env_map_parms[2]);
    env_map_parms[0] + (env_map_parms[1] - env_map_parms[0]) * t
}

/// The reflection probe's mip level for a gloss (the specular map's alpha).
pub fn probe_lod(gloss: f32) -> f32 {
    4.5 - 5.0 * gloss
}

/// The light highlight's exponent for a gloss: 8 at 0, 672 at 1.
pub fn spec_exponent(gloss: f32) -> f32 {
    (gloss * 9.377_518).exp2() + 7.0
}

/// A light's highlight on the reflection vector (`R.L`), as the shaders
/// approximate `(R.L)^n`: `min(1, e^(n (R.L - 0.99925)))`. No normalisation
/// factor: the light's specular colour and the fresnel carry the strength.
pub fn light_specular(r_dot_l: f32, gloss: f32) -> f32 {
    ((r_dot_l - 0.999_25) * spec_exponent(gloss)).exp().min(1.0)
}

/// The view vector reflected about the normal (`v` from the eye).
pub fn reflect(v: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    let d = 2.0 * (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]);
    std::array::from_fn(|i| v[i] - d * n[i])
}

/// Inputs of one lit pixel (everything linear, the textures already
/// sampled and squared where the shaders square them).
#[derive(Clone, Copy, Debug)]
pub struct LitPixel {
    /// Colour map times vertex colour, squared.
    pub albedo: [f32; 3],
    /// Specular map rgb squared, and its alpha (gloss).
    pub spec: [f32; 3],
    pub gloss: f32,
    /// Unit world normal (normal map applied), unit view vector.
    pub n: [f32; 3],
    pub v: [f32; 3],
    pub env_map_parms: [f32; 4],
    /// The reflection probe sampled along `reflect(v, n)` at `probe_lod`.
    pub probe: [f32; 3],
    /// Baked or grid light (`lightmap_light` / the light grid sample).
    pub ambient: [f32; 3],
}

/// A directional light (the sun) on a pixel: direction to it, its diffuse
/// and specular colours, and its visibility (shadow map / baked).
#[derive(Clone, Copy, Debug)]
pub struct SunLight {
    pub l: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub visibility: f32,
}

/// The lit colour: `albedo (ambient + vis N.L diffuse) + spec fresnel
/// (probe + vis highlight specular)`.
pub fn lit_colour(p: &LitPixel, sun: Option<&SunLight>) -> [f32; 3] {
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let fres = env_fresnel(p.env_map_parms, dot(p.v, p.n));
    let r = reflect(p.v, p.n);
    std::array::from_fn(|c| {
        let (mut diffuse, mut specular) = (p.ambient[c], p.probe[c]);
        if let Some(s) = sun {
            diffuse += s.visibility * dot(s.l, p.n).clamp(0.0, 1.0) * s.diffuse[c];
            specular += s.visibility * light_specular(dot(r, s.l), p.gloss) * s.specular[c];
        }
        p.albedo[c] * diffuse + p.spec[c] * fres * specular
    })
}

/// The light grid's 3D texture coordinate for a model pixel: the draw's base
/// coordinate (`BASE_LIGHTING_COORDS`) plus the normal pushed out to the unit
/// cube's shell (divided by its largest component) times
/// `LIGHTING_LOOKUP_SCALE`. The texture's rgb is the light, its alpha the
/// sun's visibility past the shadow map.
pub fn light_grid_coord(base: [f32; 3], lookup_scale: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    let m = n[0].abs().max(n[1].abs()).max(n[2].abs()).max(1e-8);
    std::array::from_fn(|i| base[i] + n[i] * lookup_scale[i] / m)
}

/// The sun shadow map's 12 filter taps, in units of
/// `SUN_SHADOWMAP_PIXEL_ADJUST.xy` (the centre tap is one of them).
pub const SUN_SHADOW_TAPS: [[f32; 2]; 12] = [
    [-2.453_568, 2.469_924],
    [2.582_72, 1.972_84],
    [0.0, 0.0],
    [-3.268_776, -1.084_384],
    [3.908_2, -0.434_46],
    [0.314_828, -2.861_292],
    [2.897_916, -2.323_192],
    [1.949_888, -0.252_328],
    [-0.336_312, 3.593_248],
    [-1.978_208, -2.844_204],
    [-3.786_372, 0.900_54],
    [0.618_06, 1.861_508],
];

/// A cascade's weight from its coordinate's extent (the larger of
/// `|uv * SHADOWMAP_SCALE.xy + SHADOWMAP_SCALE.zw|`): 1 inside, fading out
/// over the last unit before 8.
pub fn cascade_fade(extent: f32) -> f32 {
    (8.0 - extent).clamp(0.0, 1.0)
}

/// The sun's visibility from the two cascades' filtered results (each the
/// mean of the 12 taps), their fades and the baked visibility (the
/// lightmap primary, or the light grid's alpha): the far cascade over the
/// baked one by its fade, then the near cascade over that (fully wherever
/// the far cascade has started to fade inside the near one's reach).
pub fn sun_visibility(near: f32, far: f32, baked: f32, fade_near: f32, fade_far: f32) -> f32 {
    let v = baked + fade_far * (far - baked);
    let w = if fade_far * fade_near != fade_near {
        1.0
    } else {
        fade_near
    };
    v + w * (near - v)
}

/// The view distance of a `floatz` texel and the fog constants it takes:
/// above 63/64 the view model's range (near plane 0.1, `FOG_DEPTHHACK`),
/// below the world's (`ZNEAR`). `None` for 0 (nothing drawn: no fog).
pub fn floatz_depth(z: f32, znear: f32) -> Option<(f32, bool)> {
    if z == 0.0 {
        return None;
    }
    let viewmodel = z > 0.984_375;
    let (near, scale, bias) = if viewmodel {
        (0.1, 0.015_625, -0.984_375)
    } else {
        (znear, 0.984_375, 0.0)
    };
    Some((near * scale / (z + bias).max(1e-8), viewmodel))
}

/// The fog's transmittance at a distance: `FOG` = (_, 1 - max opacity,
/// -k, k start), `k = ln 2 / half distance`.
pub fn fog_transmittance(fog: [f32; 4], dist: f32) -> f32 {
    (dist * fog[2] + fog[3]).exp().max(fog[1]).clamp(0.0, 1.0)
}

/// The sun fog's share at a view direction: `FOG_SUN_CONSTS` = (-k scale,
/// cos end angle, 1 / (cos begin - cos end), -k), a linear ramp in the cosine.
pub fn sun_fog_share(sun_consts: [f32; 4], cos_to_sun: f32) -> f32 {
    ((cos_to_sun - sun_consts[1]) * sun_consts[2]).clamp(0.0, 1.0)
}

/// Depth fog with the sun fog (`postopaquefx_dfog`): the scene colour
/// through the fog, the fog's colour and transmittance both blended toward
/// the sun fog's by `sun_fog_share`.
pub fn sun_fogged(
    scene: [f32; 3],
    fog: [f32; 4],
    fog_colour: [f32; 3],
    sun_consts: [f32; 4],
    sun_colour: [f32; 3],
    dist: f32,
    cos_to_sun: f32,
) -> [f32; 3] {
    let t = (dist * sun_consts[3] + fog[3]).exp().max(fog[1]);
    let ts = (dist * sun_consts[0] + fog[3]).exp().max(fog[1]);
    let s = sun_fog_share(sun_consts, cos_to_sun);
    let t = (t + s * (ts - t)).clamp(0.0, 1.0);
    std::array::from_fn(|c| {
        let f = fog_colour[c] + s * (sun_colour[c] - fog_colour[c]);
        f + t * (scene[c] - f)
    })
}

/// The sky's fog amount at a unit direction (z up): `FOG_HEIGHT` = (min
/// angle, 1 / (max - min), intensity x max opacity), the angle taken as
/// `(1 - z) pi / 2` (0 at the zenith, pi / 2 at the horizon).
pub fn sky_fog(fog_height: [f32; 4], dir_z: f32) -> f32 {
    let a = (1.0 - dir_z) * std::f32::consts::FRAC_PI_2;
    ((a - fog_height[0]) * fog_height[1]).clamp(0.0, 1.0) * fog_height[2]
}

/// The sky pixel: the sky texture times `HDR_SKY_INTENSITY.rgb`, toward the
/// fog colour (blended with the sun fog's by `sun_fog_share`) by `sky_fog`.
pub fn sky_colour(
    sky: [f32; 3],
    sky_intensity: [f32; 3],
    fog_colour: [f32; 3],
    fog_amount: f32,
) -> [f32; 3] {
    std::array::from_fn(|c| {
        let s = sky[c] * sky_intensity[c];
        s + fog_amount * (fog_colour[c] - s)
    })
}

/// HDR colorization (`postopaquefx_*_color`, `postfx_color_grad_hdr`): Rec.
/// 601 desaturation by `power_desat.w`, then `pow(c, power.xyz) * scale +
/// bias` (a channel at or below 0 skips the power and takes only the bias).
pub fn colorization(
    c: [f32; 3],
    power_desat: [f32; 4],
    scale: [f32; 3],
    bias: [f32; 3],
) -> [f32; 3] {
    let c = c.map(|x| x.max(0.0));
    let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    std::array::from_fn(|i| {
        let x = c[i] + power_desat[3] * (l - c[i]);
        let p = if x <= 0.0 {
            0.0
        } else {
            x.powf(power_desat[i]) * scale[i]
        };
        p + bias[i]
    })
}

/// One frame of the dynamic iris: the exposure moves from `prev` toward
/// `key / max(lum, 1e-4)` by at most `rates.y dt` up and `rates.x dt` down
/// (a linear rate, exposure units per second), then is clamped to
/// [`exposure.x`, `exposure.y`]. `exposure` = TONE_MAPPING_EXPOSURE (min,
/// max, key, _), `rates` = TONE_MAPPING_WHITE_ADAPTATION_RATES (down, up,
/// dt, _).
pub fn iris_step(prev: f32, lum: f32, exposure: [f32; 4], rates: [f32; 4]) -> f32 {
    let target = exposure[2] / lum.max(1e-4);
    let d = target - prev;
    let step = if d > 0.0 {
        d.min(rates[2] * rates[1])
    } else {
        d.max(-rates[2] * rates[0])
    };
    (prev + step).min(exposure[1]).max(exposure[0])
}

/// Linear to sRGB (the exact curve), as the final pass encodes.
pub fn linear_to_srgb(x: f32) -> f32 {
    if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// The colour-grading table's coordinate: the macro table (16 entries a side)
/// is indexed by the linear tone-mapped colour, the plain grading table (32)
/// by its sRGB encoding.
pub fn clut_coord(tone_mapped: f32, macro_clut: bool) -> f32 {
    let x = tone_mapped.clamp(0.0, 1.0);
    if macro_clut {
        x * 0.9375 + 0.031_25
    } else {
        linear_to_srgb(x) * 0.968_75 + 0.015_625
    }
}

/// The black level after grading (`BLACK_LEVEL` = (level, scale, _, _)).
pub fn black_level(srgb: f32, level: [f32; 4]) -> f32 {
    ((srgb - level[0]) * level[1]).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn flat_lightmap_is_upper_plus_lower_times_dz() {
        let up = [0.2, 0.3, 0.4, 0.6];
        let lo = [1.0, 0.5, 0.25, 0.0];
        let l = lightmap_light(up, lo, tangent_normal(None));
        assert!(close(l[0], 0.2 + 0.8));
        assert!(close(l[2], 0.4 + 0.25 * 0.8));
        assert!(close(lightmap_flat_weight(0.6, 0.0), 0.8));
        // A direction past the unit circle leaves only the upper page.
        assert_eq!(lightmap_flat_weight(0.9, 0.9), 0.0);
    }

    #[test]
    fn normal_map_slopes_tilt_toward_the_baked_direction() {
        // Green is the tangent slope: tilting toward +x catches more of a
        // light baked from +x.
        let n = tangent_normal(Some([0.0, 0.25]));
        assert!(close(n[0], 0.5 / 1.25f32.sqrt()));
        let up = [0.0, 0.0, 0.0, 0.8];
        let lo = [1.0, 1.0, 1.0, 0.0];
        let flat = lightmap_light(up, lo, tangent_normal(None))[0];
        let tilted = lightmap_light(up, lo, n)[0];
        assert!(tilted > flat);
    }

    #[test]
    fn fresnel_runs_from_min_to_max() {
        let p = [0.07, 1.3, 4.0, 2.0];
        assert!(close(env_fresnel(p, 1.0), 0.07));
        assert!(close(env_fresnel(p, 0.0), 1.3));
        assert!(close(env_fresnel(p, -1.0), 0.07));
    }

    #[test]
    fn gloss_maps_to_lod_and_exponent() {
        assert!(close(probe_lod(0.9), 0.0));
        assert!(close(probe_lod(0.0), 4.5));
        assert!(close(spec_exponent(0.0), 8.0));
        assert!((spec_exponent(1.0) - 672.0).abs() < 0.5);
        assert_eq!(light_specular(1.0, 1.0), 1.0);
        // pow(x, n) ~ e^(n (x - 1)) away from the peak.
        let s = light_specular(0.99, 0.5);
        let n = spec_exponent(0.5);
        assert!(
            (s - 0.99f32.powf(n)).abs() < 0.05,
            "{s} {}",
            0.99f32.powf(n)
        );
    }

    #[test]
    fn lit_colour_sums_ambient_sun_and_reflection() {
        let p = LitPixel {
            albedo: [0.5; 3],
            spec: [0.0; 3],
            gloss: 0.5,
            n: [0.0, 0.0, 1.0],
            v: [0.0, 0.0, -1.0],
            env_map_parms: [0.1, 1.0, 2.0, 0.0],
            probe: [1.0; 3],
            ambient: [0.2; 3],
        };
        let sun = SunLight {
            l: [0.0, 0.0, 1.0],
            diffuse: [2.0; 3],
            specular: [2.5; 3],
            visibility: 0.5,
        };
        assert!(close(lit_colour(&p, Some(&sun))[0], 0.5 * (0.2 + 1.0)));
        assert!(close(lit_colour(&p, None)[0], 0.1));
    }

    #[test]
    fn grid_coord_uses_the_cube_shell() {
        let c = light_grid_coord([0.5; 3], [0.25; 3], [0.6, 0.8, 0.0]);
        assert!(close(c[1], 0.75));
        assert!(close(c[0], 0.5 + 0.25 * 0.75));
    }

    #[test]
    fn shadow_blend() {
        // Inside both cascades the near one wins; past the far one the baked
        // visibility.
        assert!(close(sun_visibility(0.2, 0.8, 1.0, 1.0, 1.0), 0.2));
        assert_eq!(sun_visibility(0.2, 0.8, 1.0, 0.0, 0.0), 1.0);
        assert_eq!(sun_visibility(0.2, 0.8, 1.0, 0.0, 1.0), 0.8);
        assert!(close(cascade_fade(7.5), 0.5));
        assert_eq!(SUN_SHADOW_TAPS.len(), 12);
    }

    #[test]
    fn fog_halves_every_half_distance_past_start() {
        let (start, half, max_op) = (100.0f32, 500.0f32, 0.9f32);
        let k = std::f32::consts::LN_2 / half;
        let fog = [0.0, 1.0 - max_op, -k, k * start];
        assert!(close(fog_transmittance(fog, 600.0), 0.5));
        assert!(close(fog_transmittance(fog, 1e7), 0.1));
        assert_eq!(fog_transmittance(fog, 50.0), 1.0);
        let fogged = sun_fogged(
            [1.0; 3],
            fog,
            [0.0; 3],
            [-k, 0.9, 10.0, -k],
            [1.0; 3],
            600.0,
            0.0,
        );
        assert!(close(fogged[0], 0.5));
    }

    #[test]
    fn floatz_depths() {
        assert_eq!(floatz_depth(0.0, 4.0), None);
        let (d, vm) = floatz_depth(0.984_375 * 4.0 / 1000.0, 4.0).unwrap();
        assert!(!vm && (d - 1000.0).abs() < 0.1);
        assert!(floatz_depth(0.99, 4.0).unwrap().1);
    }

    #[test]
    fn sky_fog_ramp() {
        let fh = [0.0, 1.0 / std::f32::consts::FRAC_PI_2, 0.5, 0.0];
        assert!(close(sky_fog(fh, 1.0), 0.0));
        assert!(close(sky_fog(fh, 0.0), 0.5));
        assert_eq!(sky_colour([1.0; 3], [2.0; 3], [0.0; 3], 0.5), [1.0; 3]);
    }

    #[test]
    fn iris_moves_at_its_rates_and_clamps() {
        let exp = [0.25, 4.0, 0.18, 1.0];
        let r = [0.5, 3.0, 0.1, 0.0];
        // Target 0.18 / 0.045 = 4: up by at most 0.3.
        assert!(close(iris_step(1.0, 0.045, exp, r), 1.3));
        assert!(close(iris_step(1.0, 18.0, exp, r), 0.95));
        assert!(close(
            iris_step(0.3, 18.0, exp, [0.5, 3.0, 10.0, 0.0]),
            0.25
        ));
    }

    #[test]
    fn grading_coords() {
        assert!(close(clut_coord(0.0, true), 0.5 / 16.0));
        assert!(close(clut_coord(1.0, true), 15.5 / 16.0));
        assert!(close(clut_coord(1.0, false), 31.5 / 32.0));
        assert!(close(black_level(0.5, [0.1, 1.25, 0.0, 0.0]), 0.5));
        let c = colorization([0.25, 0.5, 1.0], [1.0, 1.0, 1.0, 0.0], [2.0; 3], [0.0; 3]);
        assert!(close(c[1], 1.0));
    }
}
