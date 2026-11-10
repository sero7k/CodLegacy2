//! bo2mp: a Modern Warfare Remastered sprite's look by the game's own rules
//! (`fx_iw4::h1`, read off its executable: `docs/H1_NATIVE.md`, "Effects"):
//! its own random table and element seed, float colour and emissive lerped
//! with the colour random, sizes, rotation, the distance fade and draw
//! threshold, and the lit colour (HDR mode, the game's default). On by
//! default; `BO2MP_H1_FX_EXE=0` brings back the converted path (colour bytes
//! times emissive, IW4's table and lighting).

use asset_game::H1ElemSource;
use fx::FxDrawElemContext;
use fx_iw4::h1;

/// Whether Modern Warfare Remastered's elements take the game's rules.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_H1_FX_EXE").is_ok_and(|v| v.trim() == "0"))
}

/// The sprite's size, colour and rotation as the game draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct H1SpriteState {
    pub size0: f32,
    pub size1: f32,
    /// Display colour (red first), fades applied to alpha.
    pub color: [u8; 4],
    /// The colour's alpha before the fade (what sight blockers read).
    pub vis_alpha: u8,
    pub rotation: f32,
    /// The lit HDR colour before it is fitted to the sprite pass.
    pub hdr_rgb: [f32; 3],
}

/// IW4 element type of a tail (the converted element's type; Modern Warfare
/// Remastered's tails and lines both arrive as it).
const IW4_TAIL: u8 = 2;

/// The light a lit element takes: the effect's light sample (the world's
/// light at its origin, 0..255 per channel) as 0..1, white when there is
/// none. (Inferred: the game samples its HDR light grid per particle; our
/// sample is the map's packed light at the effect.)
fn light_of(ctx: &FxDrawElemContext<'_>) -> [f32; 3] {
    match ctx.packed_lighting_src {
        fx::FxPackedLightingSrc::Missing => [1.0; 3],
        _ => ctx.packed_lighting.map(|c| f32::from(c) / 255.0),
    }
}

/// `None`: the game does not draw this particle now (faded out, no alpha,
/// no size, or samples missing).
pub fn sprite_state(src: &H1ElemSource, ctx: &FxDrawElemContext<'_>, eye_dist: f32) -> Option<H1SpriteState> {
    let mut rules = src.rules;
    if ctx.elem_type == IW4_TAIL {
        // (A tail's length is its second size channel whatever its flags.)
        rules.flags |= h1::FLAG_NON_UNIFORM_SIZE;
    }
    let seed = h1::elem_seed(ctx.sequence, ctx.effect_seed, rules.random_seed, ctx.msec_begin);
    let (_, norm) = h1::draw_age(ctx.age_msec, ctx.life_msec.max(1));
    let v = h1::visual_state(&rules, &src.vis_samples, norm, seed, ctx.life_msec as f32)?;
    if v.size[0] <= 0.0 {
        return None;
    }
    let fade = h1::distance_fade(eye_dist, rules.fade_in, rules.fade_out);
    let alpha = v.rgba[3];
    if !h1::drawn(alpha, fade, 0) {
        return None;
    }
    let hdr_rgb = h1::lit_rgb(&rules, [v.rgba[0], v.rgba[1], v.rgba[2]], v.emissive, light_of(ctx), true);
    let rgb = h1::display_rgb(hdr_rgb);
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let non_uniform = rules.flags & h1::FLAG_NON_UNIFORM_SIZE != 0;
    Some(H1SpriteState {
        size0: v.size[0],
        size1: if non_uniform { v.size[1].max(0.0) } else { 0.0 },
        color: [byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), byte(alpha * fade)],
        vis_alpha: byte(alpha),
        rotation: v.rotation,
        hdr_rgb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(lit_type: u8, emissive: f32, hdr_frac: f32) -> H1ElemSource {
        let mut samples = vec![0u8; 224];
        for s in 0..2 {
            let o = s * 112;
            for (k, c) in [0.5f32, 0.25, 1.0, 0.8].into_iter().enumerate() {
                samples[o + 4 * k..o + 4 * k + 4].copy_from_slice(&c.to_le_bytes());
                samples[o + 56 + 4 * k..o + 60 + 4 * k].copy_from_slice(&c.to_le_bytes());
            }
            for k in 0..3 {
                for half in [0, 56] {
                    let at = o + half + 16 + 4 * k;
                    samples[at..at + 4].copy_from_slice(&emissive.to_le_bytes());
                }
            }
            samples[o + 36..o + 40].copy_from_slice(&12.0f32.to_le_bytes());
        }
        H1ElemSource {
            rules: h1::H1ElemRules {
                vis_intervals: 1,
                lit_type,
                hdr_lighting_frac: hdr_frac,
                fade_in: [100.0, 100.0],
                ..Default::default()
            },
            vis_samples: samples,
        }
    }

    fn ctx(src: fx::FxPackedLightingSrc, light: [u8; 3]) -> FxDrawElemContext<'static> {
        FxDrawElemContext {
            def_name: "fx/test",
            catalog_index: 0,
            def_index: 0,
            elem_type: 0,
            origin: [0.0; 3],
            axis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            frame_axis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            norm_time: 0.5,
            age_msec: 500,
            life_msec: 1000,
            elem_random_seed: 0,
            sequence: 2,
            effect_seed: 40,
            msec_begin: 1000,
            base_vel: [0.0; 3],
            flags: 0,
            at_rest_fraction: 0,
            packed_lighting: light,
            packed_lighting_src: src,
            elem_handle: 0,
        }
    }

    #[test]
    fn emissive_sprites_brighten_and_keep_their_hue() {
        // Lit type 0, emissive 4: (0.5, 0.25, 1.0) x 4, fitted to 1 at its peak.
        let st = sprite_state(&src(0, 4.0, 0.0), &ctx(fx::FxPackedLightingSrc::White, [255; 3]), 50.0).unwrap();
        assert_eq!(st.hdr_rgb, [2.0, 1.0, 4.0]);
        assert_eq!(&st.color[..3], &[128, 64, 255]);
        assert_eq!(st.color[3], 204);
        assert_eq!(st.size0, 12.0);
    }

    #[test]
    fn lit_sprites_take_the_light_times_hdr_lighting_frac() {
        let dark = sprite_state(&src(1, 0.0, 1.0), &ctx(fx::FxPackedLightingSrc::AtPointRgb, [51, 51, 51]), 50.0).unwrap();
        assert!((dark.hdr_rgb[0] - 0.5 * 0.2).abs() < 1e-6);
        assert_eq!(dark.color[0], 26);
        // No light sample: white.
        let none = sprite_state(&src(1, 0.0, 1.0), &ctx(fx::FxPackedLightingSrc::Missing, [0; 3]), 50.0).unwrap();
        assert_eq!(none.hdr_rgb, [0.5, 0.25, 1.0]);
    }

    #[test]
    fn faded_out_sprites_are_not_drawn() {
        // Past fadeIn's start + length (200 units): gone.
        assert!(sprite_state(&src(0, 1.0, 0.0), &ctx(fx::FxPackedLightingSrc::White, [255; 3]), 250.0).is_none());
        let half = sprite_state(&src(0, 1.0, 0.0), &ctx(fx::FxPackedLightingSrc::White, [255; 3]), 150.0).unwrap();
        assert_eq!(half.color[3], 102);
        assert_eq!(half.vis_alpha, 204);
    }
}
