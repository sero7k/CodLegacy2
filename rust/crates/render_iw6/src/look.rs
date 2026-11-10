//! The map's look as its zone gives it: the vision file (`vision/<map>.vision`:
//! sky intensity, film, glow, the tone mapping asset's name), the
//! createart fog (`maps/createart/<map>_fog.gsc`: the block the map's
//! own block names as its HDR override), and the tone mapping asset's 26
//! floats (`ToneMappingRec`). The code constants these become follow the
//! engine's own preparation (`docs/exe_notes/IW6_LIGHTING.md` 2 and 4).

use std::collections::HashMap;

use fastfile_iw6::walk::ToneMappingRec;

/// `key value` pairs of a vision file (lower-case keys).
#[derive(Clone, Debug, Default)]
pub struct Vision {
    pub values: HashMap<String, String>,
}

impl Vision {
    pub fn parse(text: &str) -> Self {
        let mut values = HashMap::new();
        for line in text.lines().map(str::trim) {
            let Some(split) = line.find(char::is_whitespace) else { continue };
            let (k, v) = (&line[..split], line[split..].trim().trim_matches('"').trim());
            values.insert(k.to_ascii_lowercase(), v.to_owned());
        }
        Self { values }
    }

    /// The value of `key` (compared in lower case; a short key is folded on
    /// the stack, a frame asks for a few every time).
    fn get(&self, key: &str) -> Option<&String> {
        let mut buf = [0u8; 64];
        if key.len() <= buf.len() {
            let b = &mut buf[..key.len()];
            b.copy_from_slice(key.as_bytes());
            b.make_ascii_lowercase();
            if let Ok(k) = std::str::from_utf8(b) {
                return self.values.get(k);
            }
        }
        self.values.get(&key.to_ascii_lowercase())
    }

    pub fn f(&self, key: &str, default: f32) -> f32 {
        self.get(key)
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or(default)
    }

    pub fn v3(&self, key: &str, default: [f32; 3]) -> [f32; 3] {
        self.get(key)
            .and_then(|v| {
                let p: Vec<f32> = v.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                (p.len() >= 3).then(|| [p[0], p[1], p[2]])
            })
            .unwrap_or(default)
    }

    pub fn s(&self, key: &str) -> Option<&str> {
        self.get(key).map(String::as_str)
    }
}

/// One `create_vision_set_fog` block's fields (lower-case names).
#[derive(Clone, Debug, Default)]
pub struct Fog {
    pub name: String,
    pub num: HashMap<String, f32>,
    pub vec3: HashMap<String, [f32; 3]>,
    pub text: HashMap<String, String>,
}

impl Fog {
    /// Every block of a createart fog script.
    pub fn blocks(script: &str) -> Vec<Fog> {
        let mut out = Vec::new();
        for part in script.split("create_vision_set_fog").skip(1) {
            let name = part
                .split('"')
                .nth(1)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mut f = Fog {
                name,
                ..Fog::default()
            };
            for line in part.lines() {
                let Some((k, v)) = line
                    .trim()
                    .strip_prefix("ent.")
                    .and_then(|l| l.split_once('='))
                else {
                    continue;
                };
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim().trim_end_matches(';').trim();
                if let Some(inner) = v.strip_prefix('(').and_then(|v| v.strip_suffix(')')) {
                    let p: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if p.len() == 3 {
                        f.vec3.insert(k, [p[0], p[1], p[2]]);
                    }
                } else if let Ok(n) = v.parse::<f32>() {
                    f.num.insert(k, n);
                } else {
                    f.text.insert(k, v.trim_matches('"').to_ascii_lowercase());
                }
            }
            out.push(f);
        }
        out
    }

    /// The block a map draws with: its own (named as the map), or the
    /// HDR block that one names (`HDROverride`), else the first.
    pub fn for_map(script: &str, map: &str) -> Option<Fog> {
        let blocks = Self::blocks(script);
        let map = map.to_ascii_lowercase();
        let own = blocks.iter().find(|b| b.name == map).or(blocks.first())?;
        if let Some(hdr) = own.text.get("hdroverride")
            && let Some(b) = blocks.iter().find(|b| &b.name == hdr)
        {
            return Some(b.clone());
        }
        Some(own.clone())
    }

    pub fn n(&self, k: &str, default: f32) -> f32 {
        self.num.get(k).copied().unwrap_or(default)
    }
}

/// The material bloom's constants from the vision (`r_materialBloom*`),
/// as Ghosts' bloom shaders use them (read from their translation):
/// - `material_bloom_setup_alpha_intensity`: four taps a quarter source
///   texel apart (`RENDER_TARGET_SIZE.zw`), each colour weighted by its
///   alpha (the bloom mask) / 4 and by the soft threshold
///   `1 + GLOW_SETUP.x / max(L, -GLOW_SETUP.x)` (L its luminance times
///   the mask): so `GLOW_SETUP.x` is minus the luminance cutoff; then
///   desaturated toward its luminance by `GLOW_SETUP.z`.
/// - `material_bloom_apply`: `bloom * GLOW_APPLY.w + GLOW_APPLY.x`, alpha
///   `GLOW_APPLY.y`: w the intensity.
pub fn bloom_consts(v: &Vision, scene: (u32, u32), code: &mut crate::code::CodeValues) {
    let cutoff = v.f("r_materialBloomLuminanceCutoff", 0.8).max(1e-4);
    let desat = v.f("r_materialBloomDesaturation", 0.0);
    let intensity = v.f("r_materialBloomIntensity", 0.0);
    code.set_vec("GLOW_SETUP", [-cutoff, 0.0, desat, 0.0]);
    code.set_vec("GLOW_APPLY", [0.0, 0.0, 0.0, intensity]);
    code.set_vec("RENDER_TARGET_SIZE", [scene.0 as f32, scene.1 as f32, 1.0 / scene.0 as f32, 1.0 / scene.1 as f32]);
}

/// `BLOOM_HQ_RADIUS` for one blur pass (along x or y): the blur
/// (`material_bloom_hq_blur`) takes seven taps of weight 1/7 at about
/// -1.46 .. 1.46 times this vector (uv): `r_materialBloomRadius` texels of
/// the bloom target.
pub fn bloom_radius(v: &Vision, small: (u32, u32), along_x: bool) -> [f32; 4] {
    let r = v.f("r_materialBloomRadius", 2.0);
    if along_x {
        [r / small.0 as f32, 0.0, 0.0, 0.0]
    } else {
        [0.0, r / small.1 as f32, 0.0, 0.0]
    }
}

/// The vision's HDR colorization (`colorizationSet`): its main
/// (near) grade, the alternate (far) one and the distance blend between
/// them, for the post-opaque `_color` / `_color_altd` passes.
#[derive(Clone, Debug)]
pub struct Colorization {
    pub name: String,
    pub main: fastfile_iw6::walk::ColorizationRec,
    pub alternate: Option<fastfile_iw6::walk::ColorizationRec>,
    pub depth_scale_and_bias: [f32; 2],
    /// The set's low / mid / high grade (`Lmh`), when it names all three.
    pub lmh: Option<Lmh>,
}

/// A colorization set's low / mid / high grade: what the engine bakes into
/// the final pass's colour table (`MACROCLUT`), READ from the exe's bake
/// (`docs/exe_notes/IW6_SHADERS.md` 14). For each of the 16^3 table entries
/// (input `i / 15` per channel, red fastest) and each band: desaturate
/// toward the luma (0.299, 0.587, 0.114) by the band's desaturation, raise
/// to its power per channel (0 where not positive), times its scale plus
/// its bias; the three bands blended by weights on the input's luma, the
/// sum saturated and stored as 8-bit unorm. The weights: low `1 - S(sat((L
/// - a) b))`, mid `1 - S(sat((|L - c| - d) e)) + 0.0001`, high `S(sat((L -
/// f) g))`, `S(x) = x^2 (3 - 2x)`, from the set's end points as stored
/// (`lowAndHighEndPoints` = a, b, f, g; `midEndPoints` = c, d, e).
#[derive(Clone, Debug)]
pub struct Lmh {
    pub low_and_high: [f32; 4],
    pub mid: [f32; 4],
    pub bands: [fastfile_iw6::walk::ColorizationRec; 3],
}

impl Lmh {
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        let band = |b: &fastfile_iw6::walk::ColorizationRec| -> [f32; 3] {
            let d = b.power_and_desaturation[3];
            std::array::from_fn(|k| {
                let x = c[k] * (1.0 - d) + l * d;
                let p = if x > 0.0 { x.powf(b.power_and_desaturation[k]) } else { 0.0 };
                p * b.scale[k] + b.bias[k]
            })
        };
        let s = |x: f32| x * x * (3.0 - 2.0 * x);
        let [a, b, f, g] = self.low_and_high;
        let [mc, md, me, _] = self.mid;
        let w_low = 1.0 - s(((l - a) * b).clamp(0.0, 1.0));
        let w_mid = 1.0 - s(((l - mc).abs() * me - me * md).clamp(0.0, 1.0)) + 0.0001;
        let w_high = s(((l - f) * g).clamp(0.0, 1.0));
        let inv = 1.0 / (w_low + w_mid + w_high);
        let (lo, mi, hi) = (band(&self.bands[0]), band(&self.bands[1]), band(&self.bands[2]));
        std::array::from_fn(|k| ((w_mid * mi[k] + w_low * lo[k] + w_high * hi[k]) * inv).clamp(0.0, 1.0))
    }
}

impl Colorization {
    pub fn write(&self, code: &mut crate::code::CodeValues) {
        let put = |code: &mut crate::code::CodeValues, names: [&str; 3], c: &fastfile_iw6::walk::ColorizationRec| {
            code.set_vec(names[0], c.power_and_desaturation);
            code.set_vec(names[1], [c.scale[0], c.scale[1], c.scale[2], 1.0]);
            code.set_vec(names[2], [c.bias[0], c.bias[1], c.bias[2], 0.0]);
        };
        put(code, ["COLORIZATION_POWER_AND_DESATURATION", "COLORIZATION_SCALE", "COLORIZATION_BIAS"], &self.main);
        put(
            code,
            ["COLORIZATION_POWER_AND_DESATURATION_ALTERNATE", "COLORIZATION_SCALE_ALTERNATE", "COLORIZATION_BIAS_ALTERNATE"],
            self.alternate.as_ref().unwrap_or(&self.main),
        );
        code.set_vec(
            "COLORIZATION_DEPTH_SCALE_AND_BIAS",
            [self.depth_scale_and_bias[0], self.depth_scale_and_bias[1], 0.0, 0.0],
        );
    }
}

/// The vision's film (`r_film*`), as the engine bakes it into the final
/// pass's colour table (`MACROCLUT`, `IW6_LIGHTING.md` 5: "film +
/// colorization + CLUT steps baked into one CLUT"; the colorization runs
/// in HDR before it, in the post-opaque pass). The tint curve and the
/// order (desaturate, then tint over display luminance, plus the bias, in
/// display space) are `render_h1::look::film`'s, the same engine family's
/// film pass: the quadratic Bezier from the dark tint (L 0) through the
/// medium tint to the light tint (L 1), scaled by the contrast; the bias
/// brightness + (1 - contrast) / 2; the desaturation from the dark one to
/// the light one over L. INFERRED for Ghosts (its film shader is replaced
/// by the table). `r_blacklevel` (default 0) divides the contrast by
/// `1 - blacklevel` and lowers the bias by `blacklevel / (1 - blacklevel)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Film {
    pub on: bool,
    pub base: [f32; 3],
    pub delta: [f32; 3],
    pub quad: [f32; 3],
    pub bias: f32,
    pub desat_dark: f32,
    pub desat: f32,
}

impl Film {
    pub fn from_vision(v: &Vision) -> Self {
        let on = v.f("r_filmEnable", 0.0) != 0.0;
        let one = [1.0f32; 3];
        let (dark, light) = (v.v3("r_filmDarkTint", one), v.v3("r_filmLightTint", one));
        let mid = match v.values.get("r_filmmediumtint") {
            Some(_) => v.v3("r_filmMediumTint", one),
            None => std::array::from_fn(|k| 0.5 * (dark[k] + light[k])),
        };
        let black = 0.0f32;
        let den = (1.0 - black).max(0.01);
        let mut s = v.f("r_filmContrast", 1.0) / den;
        let mut bias = v.f("r_filmBrightness", 0.0) + 0.5 - 0.5 * v.f("r_filmContrast", 1.0) - black / den;
        if v.f("r_filmInvert", 0.0) != 0.0 {
            s = -s;
            bias += 1.0;
        }
        let desat = v.f("r_filmDesaturation", 0.0);
        Self {
            on,
            base: dark.map(|d| d * s),
            delta: std::array::from_fn(|k| (mid[k] - dark[k]) * 2.0 * s),
            quad: std::array::from_fn(|k| (light[k] - 2.0 * mid[k] + dark[k]) * s),
            bias,
            desat_dark: v.f("r_filmDesaturationDark", desat),
            desat,
        }
    }

    /// The film on one display (sRGB-encoded) colour.
    pub fn apply(&self, d: [f32; 3]) -> [f32; 3] {
        if !self.on {
            return d;
        }
        let l = 0.2125 * d[0] + 0.7154 * d[1] + 0.0721 * d[2];
        let k = (self.desat_dark + (self.desat - self.desat_dark) * l).clamp(0.0, 1.0);
        std::array::from_fn(|c| {
            let x = d[c] + (l - d[c]) * k;
            (x * (self.base[c] + self.delta[c] * l + self.quad[c] * l * l) + self.bias).clamp(0.0, 1.0)
        })
    }
}

/// linear to sRGB, the exact curve.
pub fn linear_to_srgb(x: f32) -> f32 {
    if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB to linear, the engine's exact curve for light and fog colours.
pub fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.039_28 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The fog constants the engine makes of a fog block (`FOG`,
/// `FOG_COLOR_LINEAR`, `FOG_SUN_CONSTS`, `FOG_SUN_COLOR_LINEAR`,
/// `FOG_SUN_DIR`, `FOG_HEIGHT`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogConsts {
    pub fog: [f32; 4],
    pub color: [f32; 4],
    pub sun_consts: [f32; 4],
    pub sun_color: [f32; 4],
    pub sun_dir: [f32; 4],
    pub height: [f32; 4],
}

impl FogConsts {
    /// No fog: full transmittance, nothing on the sky.
    pub fn none() -> Self {
        Self {
            fog: [0.0, 1.0, 0.0, 0.0],
            color: [0.0; 4],
            sun_consts: [0.0, 1.0, 0.0, 0.0],
            sun_color: [0.0; 4],
            sun_dir: [0.0, 0.0, 1.0, 0.0],
            height: [0.0; 4],
        }
    }

    pub fn from_fog(f: &Fog) -> Self {
        let start = f.n("startdist", 0.0).max(0.0);
        let half = f.n("halfwaydist", 0.0);
        if half <= 0.0 {
            return Self::none();
        }
        let k = std::f32::consts::LN_2 / half;
        let opacity = f.n("maxopacity", 1.0).clamp(0.0, 1.0);
        let lin = |r: &str, g: &str, b: &str, i: &str| {
            let s = f.n(i, 1.0);
            [
                srgb_to_linear(f.n(r, 0.0).clamp(0.0, 1.0)) * s,
                srgb_to_linear(f.n(g, 0.0).clamp(0.0, 1.0)) * s,
                srgb_to_linear(f.n(b, 0.0).clamp(0.0, 1.0)) * s,
                1.0,
            ]
        };
        let color = lin("red", "green", "blue", "hdrcolorintensity");
        let sun_on = f.n("sunfogenabled", 0.0) > 0.0;
        let (sun_consts, sun_color, sun_dir) = if sun_on {
            let scale = f.n("normalfogscale", 1.0);
            let begin = f.n("sunbeginfadeangle", 0.0).to_radians().cos();
            let end = f.n("sunendfadeangle", 180.0).to_radians().cos();
            let ramp = if begin > end { 1.0 / (begin - end) } else { 100.0 };
            let d = f.vec3.get("sundir").copied().unwrap_or([0.0, 0.0, 1.0]);
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
            (
                [-k * scale, end, ramp, -k],
                lin("sunred", "sungreen", "sunblue", "hdrsuncolorintensity"),
                [d[0] / n, d[1] / n, d[2] / n, 0.0],
            )
        } else {
            // No sun fog: the sun share is always zero, both densities k.
            ([-k, 1.0, 0.0, -k], color, [0.0, 0.0, 1.0, 0.0])
        };
        let height = {
            let min = f.n("skyfogminangle", 0.0).to_radians();
            let mut max = f.n("skyfogmaxangle", 90.0).to_radians();
            if max <= min {
                max = min + 0.01;
            }
            [min, 1.0 / (max - min), f.n("skyfogintensity", 0.0) * opacity, 0.0]
        };
        Self {
            fog: [0.0, 1.0 - opacity, -k, k * start],
            color,
            sun_consts,
            sun_color,
            sun_dir,
            height,
        }
    }
}

/// The tone mapping asset's groups: Hable's toe/linear/shoulder (D, E, F,
/// B, C, A), the white point, the exposure (min, max, key) and the
/// adaptation rates.
#[derive(Clone, Debug, PartialEq)]
pub struct Tone {
    pub d: [f32; 3],
    pub e: [f32; 3],
    pub f: [f32; 3],
    pub b: [f32; 3],
    pub c: [f32; 3],
    pub a: [f32; 3],
    pub white: [f32; 3],
    /// (min, max, key).
    pub exposure: [f32; 3],
    /// (down, up).
    pub rates: [f32; 2],
    pub name: String,
}

impl Tone {
    /// Hable's published numbers, for a map without a tone asset.
    pub fn published() -> Self {
        Self {
            d: [0.2; 3],
            e: [0.02; 3],
            f: [0.3; 3],
            b: [0.5; 3],
            c: [0.1; 3],
            a: [0.15; 3],
            white: [11.2; 3],
            exposure: [1.0, 1.0, 0.18],
            rates: [0.0; 2],
            name: "published".into(),
        }
    }

    pub fn from_asset(t: &ToneMappingRec) -> Self {
        let g = |i: usize| [t.v[3 * i], t.v[3 * i + 1], t.v[3 * i + 2]];
        Self {
            d: g(0),
            e: g(1),
            f: g(2),
            b: g(3),
            c: g(4),
            a: g(5),
            white: g(6),
            exposure: g(7),
            rates: [t.v[24], t.v[25]],
            name: t.name.clone(),
        }
    }

    /// Hable's curve for one channel.
    pub fn curve(&self, x: f32, ch: usize) -> f32 {
        let (a, b, c, d, e, f) = (self.a[ch], self.b[ch], self.c[ch], self.d[ch], self.e[ch], self.f[ch]);
        (a * x * x + c * b * x + d * e) / (a * x * x + b * x + d * f) - e / f
    }

    /// `TONE_MAPPING_WHITE_POINT`: the scale `1 / f(W)` per channel.
    pub fn white_scale(&self) -> [f32; 3] {
        std::array::from_fn(|ch| {
            let w = self.curve(self.white[ch], ch);
            if w.abs() > 1e-6 { 1.0 / w } else { 1.0 }
        })
    }

    /// Whether the exposure adapts (the bounds differ and the rates are set).
    pub fn adaptive(&self) -> bool {
        self.exposure[0] < self.exposure[1] && self.rates[0] > 0.0 && self.rates[1] > 0.0
    }

    /// The iris step (`update_dynamic_iris_adaptation`): toward
    /// `key / mean luminance` at the rates, inside [min, max].
    pub fn iris_step(&self, prev: f32, lum: f32, dt: f32) -> f32 {
        let [min, max, key] = self.exposure;
        if !self.adaptive() {
            return 0.5 * (min + max);
        }
        let target = key / lum.max(1e-4);
        let d = target - prev;
        let step = if d > 0.0 { d.min(self.rates[1] * dt) } else { d.max(-self.rates[0] * dt) };
        (prev + step).clamp(min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAG: &str = r#"
	ent = maps\mp\_art::create_vision_set_fog( "mp_frag" );
	ent.startDist =            1500;
	ent.halfwayDist =          6145;
	ent.maxOpacity =           0.25;
	ent.HDROverride =          "mp_frag_HDR";
	ent = maps\mp\_art::create_vision_set_fog( "mp_frag_HDR" );
	ent.startDist =            1500;
	ent.halfwayDist =          6145;
	ent.red =                  0.8;
	ent.green =                0.88;
	ent.blue =                 1;
	ent.HDRColorIntensity =    1;
	ent.maxOpacity =           0.25;
	ent.sunFogEnabled =        1;
	ent.sunRed =               1;
	ent.sunGreen =             1;
	ent.sunBlue =              0.94;
	ent.HDRSunColorIntensity = 1.06;
	ent.sunDir =               (0.76, 0.38, 0.51);
	ent.sunBeginFadeAngle =    0;
	ent.sunEndFadeAngle =      100;
	ent.normalFogScale =       1;
	ent.skyFogIntensity =      1;
	ent.skyFogMinAngle =       60;
	ent.skyFogMaxAngle =       85;
"#;

    #[test]
    fn neutral_film_is_the_identity_and_frag_tints_its_shadows_blue() {
        let neutral = Film::from_vision(&Vision::parse("r_filmEnable \"1\"\n"));
        for d in [[0.0, 0.0, 0.0], [0.2, 0.5, 0.9], [1.0, 1.0, 1.0]] {
            let o = neutral.apply(d);
            for c in 0..3 {
                assert!((o[c] - d[c]).abs() < 1e-5, "{d:?} {o:?}");
            }
        }
        let frag = Film::from_vision(&Vision::parse(
            "r_filmEnable \"1\"\nr_filmDesaturation \"0.07\"\nr_filmDesaturationDark \"0.07\"\nr_filmLightTint \"1.075 1.067 1.051\"\nr_filmMediumTint \"1.043 1.059 1.075\"\nr_filmDarkTint \"1.216 1.32 1.51\"\n",
        ));
        let o = frag.apply([0.1, 0.1, 0.1]);
        assert!(o[2] > o[0] && o[0] > 0.1, "{o:?}");
    }

    #[test]
    fn the_hdr_block_is_picked_and_made_constants() {
        let f = Fog::for_map(FRAG, "mp_frag").unwrap();
        assert_eq!(f.name, "mp_frag_hdr");
        let c = FogConsts::from_fog(&f);
        let k = std::f32::consts::LN_2 / 6145.0;
        assert!((c.fog[1] - 0.75).abs() < 1e-6);
        assert!((c.fog[2] + k).abs() < 1e-9 && (c.fog[3] - k * 1500.0).abs() < 1e-6);
        // Transmittance halves one half distance past the start.
        let t = (6145.0f32 + 1500.0) * c.fog[2] + c.fog[3];
        assert!((t.exp() - 0.5).abs() < 1e-4);
        assert!((c.sun_color[2] - srgb_to_linear(0.94) * 1.06).abs() < 1e-6);
        assert!((c.height[2] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn frag_tone_white_scale_maps_white_to_one() {
        let mut v = [0f32; 26];
        let groups: [[f32; 3]; 8] = [[0.8; 3], [-2.0; 3], [10.0; 3], [3.0; 3], [1.0; 3], [1.0; 3], [16.0; 3], [0.4, 2.2, 1.1]];
        for (i, g) in groups.iter().enumerate() {
            v[3 * i..3 * i + 3].copy_from_slice(g);
        }
        v[24] = 1.0;
        v[25] = 1.0;
        let t = Tone::from_asset(&ToneMappingRec { name: "mp_frag_tone".into(), v });
        let w = t.white_scale();
        assert!((t.curve(16.0, 0) * w[0] - 1.0).abs() < 1e-5);
        assert!(t.adaptive());
        // Brightening is limited by the up rate per second.
        let e = t.iris_step(1.0, 0.1, 0.1);
        assert!((e - 1.1).abs() < 1e-5, "{e}");
    }
}

#[cfg(test)]
mod lmh_tests {
    use super::Lmh;
    use fastfile_iw6::walk::ColorizationRec;

    fn rec(power: f32, desat: f32, scale: f32, bias: f32) -> ColorizationRec {
        ColorizationRec { name: String::new(), power_and_desaturation: [power, power, power, desat], scale: [scale; 3], bias: [bias; 3] }
    }

    #[test]
    fn identity_bands_give_the_input_back() {
        let l = Lmh { low_and_high: [0.2, 5.0, 0.6, 5.0], mid: [0.4, 0.1, 5.0, 0.0], bands: [rec(1.0, 0.0, 1.0, 0.0), rec(1.0, 0.0, 1.0, 0.0), rec(1.0, 0.0, 1.0, 0.0)] };
        for c in [[0.0, 0.0, 0.0], [0.2, 0.5, 0.9], [1.0, 1.0, 1.0]] {
            let o = l.apply(c);
            for k in 0..3 {
                assert!((o[k] - c[k]).abs() < 1e-5, "{c:?} -> {o:?}");
            }
        }
    }

    #[test]
    fn the_bands_weigh_by_luma() {
        // Low band black, high band white, mid band grey: dark inputs go to
        // the low band, light ones to the high band.
        let l = Lmh { low_and_high: [0.2, 5.0, 0.6, 5.0], mid: [0.4, 0.1, 5.0, 0.0], bands: [rec(1.0, 0.0, 0.0, 0.0), rec(1.0, 0.0, 0.0, 0.5), rec(1.0, 0.0, 0.0, 1.0)] };
        assert!(l.apply([0.0; 3])[0] < 0.01);
        assert!(l.apply([1.0; 3])[0] > 0.99);
        assert!((l.apply([0.4; 3])[0] - 0.5).abs() < 0.01);
    }
}

