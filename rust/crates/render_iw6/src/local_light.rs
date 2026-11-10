//! A spot or omni primary light's constants for the `ls0` / `lo0` lit
//! techniques, as `render_h1::local_light` builds MWR's (the same engine
//! family; `docs/H1_NATIVE.md`, "Spot and omni lights"): position
//! eye-relative with `1 / radius` in w, diffuse and specular the colour
//! times the sun's scales, the spot direction, the spot factors
//! `(1 / (cosInner - cosOuter), -cosOuter / (cosInner - cosOuter),
//! exponent, 0)`. `LIGHT_FALLOFF_PLACEMENT` is how Ghosts' shaders read
//! the falloff; see `falloff_placement`.

use glam::Vec3;

use crate::code::CodeValues;
use crate::scene::PrimaryLight;

/// `BO2MP_IW6_LIGHT_FALLOFF=0`: the inferred ramp `1 - d / r` instead of
/// the light defs' own falloff ramps (`patch_falloff`).
pub fn falloff_read() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_LIGHT_FALLOFF").is_ok_and(|v| v.trim() == "0"))
}

/// The falloff ramp's length in texels when the def's attenuation picture
/// is not in the zones: mp_frag's three defs sit 34 texels apart (a 32
/// texel ramp and its two edge texels), and the one whose picture is
/// loaded is 32 wide.
pub const DEFAULT_FALLOFF_WIDTH: u16 = 32;

/// `LIGHT_FALLOFF_PLACEMENT`. The light shaders (read from their
/// translation) take `t = saturate(distance / radius)`, look the falloff
/// up at `t * PLACEMENT.xy + PLACEMENT.zw` in the lightmap secondary
/// (`t3`) and square it. The ramps are in the pages: the light compiler
/// writes each light def's attenuation picture into row 0 of the upper
/// page, starting at the def's `lmapLookupStart` texel, with its edge
/// texels repeated on both sides (read from mp_frag's pages: defs at 1, 35
/// and 69, 32 texels each, falling from 0.99 to 0.02). So the placement is
/// `(width / W, 0, lmapLookupStart / W, 0)` for a page `W` texels wide: the
/// ramp runs from the left edge of its first texel to the right edge of
/// its last, on the top row (IW4's form, whose pages were 512 wide).
/// `None`: no def, or `BO2MP_IW6_LIGHT_FALLOFF=0` (the shader's lookup is
/// then replaced by `1 - t`, `patch_falloff`).
pub fn falloff_placement(l: &PrimaryLight, page_width: u32) -> Option<[f32; 4]> {
    let start = l.lookup_start.filter(|_| falloff_read() && page_width > 0)?;
    let w = page_width as f32;
    let len = f32::from(l.falloff_width.unwrap_or(DEFAULT_FALLOFF_WIDTH));
    Some([len / w, 0.0, start as f32 / w, 0.0])
}

/// Replaces a light shader's falloff lookup by the ramp `1 - t`: the
/// statement that applies `cb{slot}.d[{dest}u]` (the placement) to `t`
/// writes a register; the first texture sample at that register after it
/// becomes `vec3(saturate(1 - reg.x))`. Returns whether it was found.
pub fn patch_falloff(wgsl: &mut String, slot: u32, dest: u16) -> bool {
    let needle = format!("cb{slot}.d[{dest}u].zw)");
    let Some(at) = wgsl.find(&needle) else { return false };
    // The register the statement writes: `        rN.x = bitcast<u32>(t.x);`.
    let rest = &wgsl[at..];
    let Some(eq) = rest.find(".x = bitcast<u32>(t.x)") else { return false };
    let line_start = rest[..eq].rfind('\n').map_or(0, |i| i + 1);
    let reg = rest[line_start..eq].trim().to_string();
    if !reg.starts_with('r') {
        return false;
    }
    let coord = format!(", bitcast<vec2<f32>>({reg}.xy))).xyz");
    let from = at + eq;
    let Some(rel) = wgsl[from..].find(&coord) else { return false };
    let end = from + rel + coord.len();
    let Some(open) = wgsl[from..from + rel].rfind("(textureSample(") else { return false };
    let start = from + open;
    wgsl.replace_range(start..end, &format!("vec3<f32>(saturate(1.0 - bitcast<f32>({reg}.x)))"));
    true
}

/// Writes light `l` as the pass's primary light (`LIGHT_*`), with its
/// shadow map when it has one this frame (`spot_shadow`: the lookup, the
/// tap spacing and the fade `LIGHT_SPOTFACTORS.w` 1; else the fade 0, the
/// baked visibility).
pub fn write(l: &PrimaryLight, eye: Vec3, page_width: u32, shadow: Option<&crate::spot_shadow::SpotShadow>, code: &mut CodeValues) {
    let o = Vec3::from(l.origin) - eye;
    let c = Vec3::from(l.color);
    static KD: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_DIFFUSE_SCALE", 1.0);
    static KS: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_SPECULAR_SCALE", 2.5);
    let (kd, ks) = (KD.get(), KS.get());
    let range = (l.cos_half_fov_inner - l.cos_half_fov_outer).max(1e-6);
    code.set_vec("LIGHT_POSITION", [o.x, o.y, o.z, 1.0 / l.radius.max(1e-3)]);
    code.set_vec("LIGHT_DIFFUSE", [c.x * kd, c.y * kd, c.z * kd, 1.0]);
    code.set_vec("LIGHT_SPECULAR", [c.x * ks, c.y * ks, c.z * ks, 1.0]);
    code.set_vec("LIGHT_SPOTDIR", [l.dir[0], l.dir[1], l.dir[2], 0.0]);
    code.set_vec(
        "LIGHT_SPOTFACTORS",
        [1.0 / range, -l.cos_half_fov_outer / range, f32::from(l.exponent), if shadow.is_some() { 1.0 } else { 0.0 }],
    );
    code.set_vec("LIGHT_FALLOFF_PLACEMENT", falloff_placement(l, page_width).unwrap_or([1.0, 0.0, 0.0, 0.0]));
    if let Some(s) = shadow {
        code.set_matrix("TRANSPOSE_SHADOW_LOOKUP_MATRIX", s.lookup);
        code.set_vec("SPOT_SHADOWMAP_PIXEL_ADJUST", s.pixel_adjust);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_falloff_lookup_is_replaced() {
        let mut w = String::from(
            "    { let t = (bitcast<vec2<f32>>(r0.ww) * bitcast<vec2<f32>>(cb1.d[4u].xy) + bitcast<vec2<f32>>(cb1.d[4u].zw));\n        r2.x = bitcast<u32>(t.x);\n        r2.y = bitcast<u32>(t.y);\n    }\n    { let t = (textureSample(t3, s3, bitcast<vec2<f32>>(r2.xy))).xyz;\n",
        );
        assert!(patch_falloff(&mut w, 1, 4));
        assert!(w.contains("{ let t = vec3<f32>(saturate(1.0 - bitcast<f32>(r2.x)));"), "{w}");
        assert!(!patch_falloff(&mut String::new(), 1, 4));
    }

    #[test]
    fn a_spot_light_fills_its_constants() {
        let l = PrimaryLight {
            ty: 2,
            exponent: 1,
            color: [2.0, 1.0, 0.5],
            dir: [0.0, 0.0, -1.0],
            origin: [110.0, 20.0, 30.0],
            radius: 200.0,
            cos_half_fov_outer: 0.5,
            cos_half_fov_inner: 0.9,
            ..PrimaryLight::default()
        };
        let mut c = CodeValues::default();
        write(&l, Vec3::new(10.0, 20.0, 30.0), 1024, None, &mut c);
        assert_eq!(c.get("LIGHT_POSITION").unwrap()[0], [100.0, 0.0, 0.0, 1.0 / 200.0]);
        let f = c.get("LIGHT_SPOTFACTORS").unwrap()[0];
        assert!((f[0] - 2.5).abs() < 1e-5 && (f[1] + 1.25).abs() < 1e-5);
        assert_eq!(f[3], 0.0);
    }

    #[test]
    fn the_falloff_is_placed_at_the_defs_ramp() {
        let l = PrimaryLight {
            ty: 2,
            lookup_start: Some(35),
            falloff_width: Some(32),
            ..PrimaryLight::default()
        };
        let p = falloff_placement(&l, 1024).unwrap();
        // t = 0: the left edge of texel 35; t = 1: the right edge of texel 66.
        assert!((p[2] * 1024.0 - 35.0).abs() < 1e-4);
        assert!(((p[0] + p[2]) * 1024.0 - 67.0).abs() < 1e-4);
        assert_eq!((p[1], p[3]), (0.0, 0.0));
        assert!(falloff_placement(&PrimaryLight::default(), 1024).is_none());
    }
}
