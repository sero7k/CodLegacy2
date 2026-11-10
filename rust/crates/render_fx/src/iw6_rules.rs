//! bo2mp: a Ghosts sprite's look by Ghosts' own rules (`fx_iw4::iw6`, from
//! `docs/exe_notes/IW6_VFX_AUDIO.md`, part A): its element seed, colour and
//! unlit HDR scale lerped with the colour random, sizes, rotation, the
//! distance fade and draw threshold, and (with `BO2MP_IW6_FX_LIT=1`) the lit
//! blend of lit types 3 and 4. On by default; `BO2MP_IW6_FX_RULES=0` brings
//! back the converted path (IW4's seed and table, the unlit scale dropped).
//! On a Ghosts map drawn by `render_iw6` the HDR colour goes through that
//! frame's grade (`BO2MP_IW6_FX_GRADE`, `fx_iw4::iw6::grade`).

use asset_game::Iw6ElemSource;
use fx::FxDrawElemContext;
use fx_iw4::iw6;

/// Whether Ghosts' elements take Ghosts' rules (`BO2MP_IW6_FX_RULES`).
pub fn on() -> bool {
    asset_game::iw6_fx_rules_on()
}

/// Whether lit types 3 and 4 take Ghosts' lit blend (`BO2MP_IW6_FX_LIT`).
pub fn lit_on() -> bool {
    asset_game::iw6_fx_lit_on()
}

/// `BO2MP_IW6_FX_TAIL_SIZE` (on): a tail's second size channel (its length)
/// is read only when the element has the non-uniform size flag, as the exe's
/// size evaluation does for every element type (the length equals the first
/// size otherwise); off: always read, as before (`docs/exe_notes/IW6_FX_EXE.md`).
pub fn tail_size_by_flag_on() -> bool {
    asset_audio::iw6_rules::switch("FX_TAIL_SIZE", true)
}

/// The sprite's size, colour and rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iw6SpriteState {
    pub size0: f32,
    pub size1: f32,
    /// Display colour (red first), the distance fade applied to alpha.
    pub color: [u8; 4],
    /// The colour's alpha before the fade (what sight blockers read).
    pub vis_alpha: u8,
    pub rotation: f32,
    /// The colour took the light by Ghosts' blend (the converted lighting
    /// is then not applied).
    pub lit: bool,
}

/// IW4 element type of a tail (Ghosts' tails arrive as it).
const IW4_TAIL: u8 = 2;

/// The light a lit element takes: the effect's light sample (0..255 per
/// channel) as 0..1, white when there is none.
fn light_of(ctx: &FxDrawElemContext<'_>) -> [f32; 3] {
    match ctx.packed_lighting_src {
        fx::FxPackedLightingSrc::Missing => [1.0; 3],
        _ => ctx.packed_lighting.map(|c| f32::from(c) / 255.0),
    }
}

/// `None`: the sprite is not drawn now (faded out, no alpha, no size, or
/// samples missing). `lit`: apply the lit blend to lit types 3 and 4.
pub fn sprite_state(src: &Iw6ElemSource, ctx: &FxDrawElemContext<'_>, eye_dist: f32, lit: bool) -> Option<Iw6SpriteState> {
    let mut rules = src.rules;
    if ctx.elem_type == IW4_TAIL && !tail_size_by_flag_on() {
        // (The old reading: a tail's length is its second size channel
        // whatever its flags.)
        rules.flags |= iw6::FLAG_NON_UNIFORM_SIZE;
    }
    let seed = iw6::elem_seed(ctx.sequence, ctx.effect_seed, rules.random_seed, ctx.msec_begin);
    let (_, norm) = iw6::draw_age(ctx.age_msec, ctx.life_msec.max(1));
    let v = iw6::visual_state(&rules, &src.vis_samples, norm, seed, ctx.life_msec as f32)?;
    if v.size[0] <= 0.0 {
        return None;
    }
    let fade = iw6::distance_fade(eye_dist, rules.fade_in, rules.fade_out);
    let alpha = v.rgba[3];
    // (fx_alphaThreshold: 0 by default.)
    if !iw6::drawn(alpha, fade, 0) {
        return None;
    }
    let rgb = [v.rgba[0], v.rgba[1], v.rgba[2]];
    // (The effect's scene scale is not known here: 1.)
    let scene = iw6::scene_scale(rules.flags, 1.0);
    let lit = lit && iw6::takes_light(rules.lit_type);
    let hdr = if lit {
        iw6::lit_rgb(&rules, rgb, v.unlit_scale, light_of(ctx), scene)
    } else {
        iw6::unlit_rgb(rgb, v.unlit_scale, scene)
    };
    // On a Ghosts map drawn natively, the frame's own grade (its iris
    // exposure, tone curve and film: `asset_world::iw6_fx_grade`); else the
    // converted path's scale-down.
    let rgb = match asset_world::iw6_fx_grade() {
        Some(g) => g.apply(hdr),
        None => iw6::display_rgb(hdr),
    };
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let non_uniform = rules.flags & iw6::FLAG_NON_UNIFORM_SIZE != 0;
    Some(Iw6SpriteState {
        size0: v.size[0],
        size1: if non_uniform { v.size[1].max(0.0) } else { 0.0 },
        color: [byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), byte(alpha * fade)],
        vis_alpha: byte(alpha),
        rotation: v.rotation,
        lit,
    })
}
