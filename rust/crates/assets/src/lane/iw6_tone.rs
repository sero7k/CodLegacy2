//! bo2mp: Ghosts' own tone mapping, read from the map's zone, as the target
//! the Black Ops III look is set to (`super::iw6_map`, `super::bo3_look`).
//!
//! **What the zone holds.** Each map's vision (`vision/<map>.vision`, a text
//! rawfile) names a `toneMapping` asset (`mp_frag_tone`) that sits in the
//! map's zone: 8 groups of three floats and two adaptation rates
//! (`fastfile_iw6::walk::ToneMappingRec`). The groups are named by the
//! header's layout (and the engine's own constant preparation, read from the
//! executable) as the parameters of Hable's filmic curve
//! `f(x) = (x (A x + C B) + D E) / (x (A x + B) + D F) - E / F` (A the
//! shoulder strength, B the linear strength, C the linear angle, D the toe
//! strength, E and F the toe angle's numerator and denominator), then the
//! white point and the exposure. READ: the numbers (mp_frag_tone: D 0.8,
//! E -2, F 10, B 3, C 1, A 1, white 16, exposure 0.4 / 2.2 / 1.1, rates 1 and
//! 1). READ from the executable: the group order (D, E, F, B, C, A, white
//! point, each a red/green/blue triple), that the curve runs per channel with
//! `1 / f(white)` as the scale, and that the three exposure floats are
//! (min, max, key): the exposure adapts between min and max when min < max
//! and both rates are above zero, else it is held at (min + max) / 2. READ
//! from Ghosts' own iris shader (`docs/exe_notes/IW6_SHADERS.md`): it moves
//! the exposure toward key / mean luminance at a linear rate (the second rate
//! up, the first down), clamped to [min, max]. INFERRED:
//! that the curve is applied as `f(x e) / f(W)` followed by a 2.2 gamma. The check that
//! supports it: with those readings a mid-grey (albedo 0.35) under mp_frag's
//! sun (8) and baked light draws 0.65, the middle of the band the look check
//! calls right, and Prison Break's draws 0.74, from numbers nothing here
//! tuned. With `BO2MP_IW6_IRIS=0` only the first exposure float is used
//! (the iris's floor, where a bright sunlit frame settles).
//!
//! **How it is used.** The Black Ops III branch's tone curve is the shader's
//! (`t7_tone`), not this one. The map's curve sets the *targets* the branch is
//! solved to: what a mid-grey draws in the sun and in the shade under the map's
//! own curve and light (the HDR sun peak and the baked shaded level), then the
//! sun's strength and the exposure that give the branch's curve the same two
//! values. `BO2MP_IW6_TONEMAP=bo3` keeps the earlier rule (a fixed 0.65 and the
//! sun ratio of `bo3_look::map_sun_ratio`); `hable` uses this.

use std::collections::HashMap;

use fastfile_iw6::walk::ToneMappingRec;

/// Which rule sets the sun's strength and the exposure's target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// The earlier rule: a fixed mid-grey target and the sun ratio from the
    /// sun peak over the baked shade.
    Bo3,
    /// The map's own curve, light and exposure give both targets.
    Hable,
}

/// The mode when `BO2MP_IW6_TONEMAP` is not set.
pub(super) const DEFAULT_MODE: Mode = Mode::Hable;

/// `BO2MP_IW6_TONEMAP` (`hable` or `bo3`; `default` when unset or unreadable).
pub(super) fn mode(default: Mode) -> Mode {
    match std::env::var("BO2MP_IW6_TONEMAP").ok().map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("hable") => Mode::Hable,
        Some("bo3") => Mode::Bo3,
        _ => default,
    }
}

/// The mid-grey albedo and the display gamma the targets are taken with
/// (`bo3_look::look_check`'s grey; 2.2 is INFERRED).
const GREY: f32 = 0.35;
const GAMMA: f32 = 2.2;

/// Hable's filmic curve with its exposure.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Hable {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
    pub white: f32,
    pub exposure: f32,
    /// The exposure floats as stored: the adaptation's lower and upper bound
    /// and the key the iris holds the frame's mean luminance at.
    pub exposure_min: f32,
    pub exposure_max: f32,
    pub exposure_third: f32,
    /// The two adaptation rates as stored.
    pub rates: [f32; 2],
    /// Where the numbers came from (for the report).
    pub source: String,
    /// Read from the zone's asset (not Hable's published defaults).
    pub from_zone: bool,
}

/// Whether the grade takes the map's curve per colour channel
/// (`BO2MP_IW6_TONE_RGB`; on by default, `=0` turns it off): the table Black Ops II's final
/// pass looks the frame up in moves each channel from the one curve the
/// look is set to (`Hable::from_asset`, the groups averaged by luminance)
/// to that channel's own (`channel_lut`).
pub(super) fn rgb_on() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_TONE_RGB", true)
}

/// Whether the exposure the map's curve is taken at is the one its iris
/// settles at (`BO2MP_IW6_IRIS`; on by default; `=0` takes the first exposure
/// float, as before). See `Hable::iris_exposure`.
pub(super) fn iris_on() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_IRIS", true)
}


impl Hable {
    /// Hable's published defaults (Uncharted 2: A 0.15, B 0.50, C 0.10, D 0.20,
    /// E 0.02, F 0.30, white 11.2), for a map whose zone has no tone mapping
    /// asset. The exposure is then solved (`with_solved_exposure`).
    pub fn published() -> Self {
        Self { a: 0.15, b: 0.5, c: 0.1, d: 0.2, e: 0.02, f: 0.3, white: 11.2, exposure: 1.0, exposure_min: 1.0, exposure_max: 1.0, exposure_third: 1.0, rates: [0.0; 2], source: "Hable's published defaults (the zone has no tone mapping asset)".to_owned(), from_zone: false }
    }

    /// From an asset's 26 floats, each group of three averaged by luminance
    /// weights (Prison Break's linear strength differs in blue only).
    pub fn from_asset(t: &ToneMappingRec) -> Option<Self> {
        Self::from_groups(t, |i: usize| 0.2126 * t.v[3 * i] + 0.7152 * t.v[3 * i + 1] + 0.0722 * t.v[3 * i + 2])
    }

    /// One colour channel's curve (0 red, 1 green, 2 blue) from an asset:
    /// the engine runs the curve per channel, each with its own six
    /// parameters and white point; the exposure is shared.
    pub fn from_asset_channel(t: &ToneMappingRec, channel: usize) -> Option<Self> {
        let ch = channel.min(2);
        let mut h = Self::from_groups(t, |i: usize| t.v[3 * i + ch])?;
        h.source = format!("{} ({} channel)", h.source, ["red", "green", "blue"][ch]);
        Some(h)
    }

    fn from_groups(t: &ToneMappingRec, g: impl Fn(usize) -> f32) -> Option<Self> {
        let h = Self {
            d: g(0),
            e: g(1),
            f: g(2),
            b: g(3),
            c: g(4),
            a: g(5),
            white: g(6),
            exposure: t.v[21],
            exposure_min: t.v[21],
            exposure_max: t.v[22],
            exposure_third: t.v[23],
            rates: [t.v[24], t.v[25]],
            source: format!("the zone's tone mapping asset {}", t.name),
            from_zone: true,
        };
        let all = [h.a, h.b, h.c, h.d, h.e, h.f, h.white, h.exposure];
        (all.iter().all(|v| v.is_finite()) && h.white > 0.0 && h.exposure > 0.0 && h.f.abs() > 1e-6).then_some(h)
    }

    /// Whether the engine adapts the exposure between its bounds: the bounds
    /// differ and both rates are above zero. Otherwise it holds the middle.
    pub fn adaptive(&self) -> bool {
        self.exposure_min < self.exposure_max && self.rates[0] > 0.0 && self.rates[1] > 0.0
    }

    /// The exposure the engine holds when it does not adapt.
    pub fn held_exposure(&self) -> f32 {
        0.5 * (self.exposure_min + self.exposure_max)
    }

    /// The exposure an adapting scene settles at for a scene whose metered
    /// average is `average` (linear): the one that brings the average to
    /// `target`, kept inside the bounds. A fixed-exposure map returns its
    /// held value.
    pub fn settled_exposure(&self, average: f32, target: f32) -> f32 {
        if !self.adaptive() {
            return self.held_exposure();
        }
        if !(average > 0.0 && average.is_finite()) {
            return self.exposure_max;
        }
        (target / average).clamp(self.exposure_min, self.exposure_max)
    }

    /// The exposure the engine's iris settles at for a frame whose metered
    /// mean luminance is `average` (linear): `settled_exposure` toward the
    /// asset's key (where the iris's linear steps come to rest).
    /// A curve with no zone asset keeps its solved exposure (its bounds and
    /// rates are not known).
    pub fn iris_exposure(&self, average: f32) -> f32 {
        if self.from_zone { self.settled_exposure(average, self.exposure_third) } else { self.exposure }
    }

    /// The scene light (linear) a display value `d` comes from at
    /// `exposure` (bisection; the curve rises to 1 at the white point).
    pub fn inverse_display(&self, d: f32, exposure: f32) -> f32 {
        if !(d > 0.0) || !(exposure > 0.0) {
            return 0.0;
        }
        let top = self.white / exposure;
        if d >= 1.0 {
            return top;
        }
        let (mut lo, mut hi) = (0.0f32, top);
        for _ in 0..48 {
            let mid = 0.5 * (lo + hi);
            if self.display_at(mid, exposure) < d { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    }

    /// The curve alone, before the white point.
    pub fn curve(&self, x: f32) -> f32 {
        ((x * (self.a * x + self.c * self.b) + self.d * self.e) / (x * (self.a * x + self.b) + self.d * self.f)) - self.e / self.f
    }

    /// Scene light (linear) to a display value 0..1 under `exposure`.
    pub fn display_at(&self, x: f32, exposure: f32) -> f32 {
        let white = self.curve(self.white);
        if !white.is_finite() || white <= 1e-6 {
            return 0.0;
        }
        let v = self.curve((x * exposure).max(0.0)) / white;
        if v.is_finite() { v.clamp(0.0, 1.0).powf(1.0 / GAMMA) } else { 0.0 }
    }

    pub fn display(&self, x: f32) -> f32 {
        self.display_at(x, self.exposure)
    }

    /// The same curve with the exposure at which a mid-grey in the sun of
    /// `lit` (scene light) draws `target` (bisection; the curve rises with it).
    pub fn with_solved_exposure(mut self, lit: f32, target: f32) -> Self {
        let (mut lo, mut hi) = (1e-3f32, 1e3f32);
        for _ in 0..60 {
            let mid = (lo * hi).sqrt();
            if self.display_at(GREY * lit, mid) < target { lo = mid } else { hi = mid }
        }
        self.exposure = (lo * hi).sqrt();
        self
    }
}

/// The curve for a map: the asset its vision names, else Hable's defaults
/// with the exposure solved for a mid-grey in the sun at `fallback_grey`.
pub(super) fn curve_for(vision: &Vision, tones: &HashMap<String, ToneMappingRec>, sun_plus_shade: f32, fallback_grey: f32) -> Hable {
    vision
        .tone_mapping
        .as_ref()
        .and_then(|n| tones.get(&n.to_ascii_lowercase()))
        .and_then(Hable::from_asset)
        .unwrap_or_else(|| Hable::published().with_solved_exposure(sun_plus_shade, fallback_grey))
}

/// The three channels' own curves of the asset the map's vision names
/// (`None` without one: Hable's defaults are the same in every channel).
pub(super) fn channels_for(vision: &Vision, tones: &HashMap<String, ToneMappingRec>) -> Option<[Hable; 3]> {
    let t = tones.get(&vision.tone_mapping.as_ref()?.to_ascii_lowercase())?;
    Some([Hable::from_asset_channel(t, 0)?, Hable::from_asset_channel(t, 1)?, Hable::from_asset_channel(t, 2)?])
}

/// Side of the final pass's colour table (`asset_world::T6_LUT_SIZE`).
const LUT: usize = asset_world::T6_LUT_SIZE;

/// The final pass's colour table (32^3 RGBA8, red fastest) that moves each
/// channel from the one curve the look is matched to (`lum`) to that
/// channel's own: a display value `d` of a channel is taken back to its scene
/// light under `lum` and drawn again under the channel's curve, both at
/// `exposure`. With the same curve in every channel it is the identity. A
/// film vision (`film`), when it is drawn as well, follows it in the same
/// table (Ghosts grades its film after the tone map).
pub(super) fn channel_lut(lum: &Hable, channels: &[Hable; 3], exposure: f32, film: Option<&asset_world::FilmVision>) -> Vec<u8> {
    let ramp: [[f32; LUT]; 3] = std::array::from_fn(|c| {
        std::array::from_fn(|i| {
            let d = i as f32 / (LUT - 1) as f32;
            channels[c].display_at(lum.inverse_display(d, exposure), exposure)
        })
    });
    let mut out = Vec::with_capacity(LUT * LUT * LUT * 4);
    for b in 0..LUT {
        for g in 0..LUT {
            for r in 0..LUT {
                let mut v = [ramp[0][r], ramp[1][g], ramp[2][b]];
                if let Some(f) = film {
                    v = asset_world::iw_film_colour(f, v);
                }
                out.extend(v.map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8));
                out.push(255);
            }
        }
    }
    out
}

/// How far a channel's curve moves a mid-grey (display value 0.5) from the
/// look's one curve: the three channels' values (for the report).
pub(super) fn channel_spread(lum: &Hable, channels: &[Hable; 3], exposure: f32) -> [f32; 3] {
    let x = lum.inverse_display(0.5, exposure);
    std::array::from_fn(|c| channels[c].display_at(x, exposure))
}

/// What the map's curve says, and what the branch is set to match it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Solution {
    /// A mid-grey in the sun / in the shade, under the map's curve.
    pub grey_lit: f32,
    pub grey_shade: f32,
    /// The sun's light in the branch's units and the exposure that give the
    /// branch's curve those two values over the baked shade (`shade`).
    pub sun_light: f32,
    pub exposure: f32,
    /// The sun's ratio over the shade was held to the range the look allows.
    pub clamped: bool,
}

/// The two display targets under `curve` (the HDR sun `sun_peak` and the baked
/// shaded level `shade_raw`, Ghosts' linear units), then the branch's sun and
/// exposure for them over `shade` (the same shade after the look's scale).
/// `None` when the branch's curve cannot reach a target.
pub(super) fn solve(curve: &Hable, sun_peak: f32, shade_raw: f32, shade: f32) -> Option<Solution> {
    if !(sun_peak > 0.0 && shade_raw > 0.0 && shade > 0.0) {
        return None;
    }
    let grey_lit = curve.display(GREY * (sun_peak + shade_raw));
    let grey_shade = curve.display(GREY * shade_raw);
    // The band the look check calls right, so a curve whose own numbers fall
    // outside it (a stylised one) does not push the branch out of range.
    let target_lit = grey_lit.clamp(0.5, 0.85);
    let target_shade = grey_shade.clamp(0.14, 0.55);
    // The exposure that puts the shade at its target, then the sun that puts the
    // grey in the sun at its own.
    let bisect = |f: &dyn Fn(f32) -> f32, target: f32, lo: f32, hi: f32| -> Option<f32> {
        if f(hi) < target || f(lo) > target {
            return None;
        }
        let (mut lo, mut hi) = (lo, hi);
        for _ in 0..50 {
            let mid = (lo * hi).sqrt();
            if f(mid) < target { lo = mid } else { hi = mid }
        }
        Some((lo * hi).sqrt())
    };
    let exposure = bisect(&|e| super::bo3_look::tone(GREY * shade, e), target_shade, 0.02, 64.0)?;
    let total = bisect(&|s| super::bo3_look::tone(GREY * s, exposure), target_lit, shade * 0.5, shade * 400.0)?;
    let raw_sun = (total - shade).max(0.0);
    // The same range the earlier rule's ratio is held to (1.07..3.55 times the shade).
    let ratio = (raw_sun / shade).clamp(1.07, 3.55);
    Some(Solution { grey_lit, grey_shade, sun_light: ratio * shade, exposure, clamped: (ratio * shade - raw_sun).abs() > 1e-4 * shade })
}

/// What a map's vision says that bears on its light (`vision/<map>.vision`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Vision {
    pub tone_mapping: Option<String>,
    pub colorization_set: Option<String>,
    pub hdr_sky_intensity: Option<f32>,
    pub lit_surface_scalar: Option<f32>,
    pub unlit_surface_scalar: Option<f32>,
    pub film_tints: Option<[[f32; 3]; 3]>,
    pub desaturation: Option<f32>,
    pub bloom_cutoff: Option<f32>,
    pub bloom_intensity: Option<f32>,
    pub material_bloom_intensity: Option<f32>,
}

impl Vision {
    pub fn parse(text: &str) -> Self {
        let mut v = Self::default();
        let (mut light, mut medium, mut dark) = (None, None, None);
        for line in text.lines().map(str::trim) {
            let Some(split) = line.find(char::is_whitespace) else { continue };
            let (key, val) = (&line[..split], line[split..].trim().trim_matches('"').trim());
            let one = || val.parse::<f32>().ok();
            let three = || {
                let p: Vec<f32> = val.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                (p.len() == 3).then(|| [p[0], p[1], p[2]])
            };
            match key.to_ascii_lowercase().as_str() {
                "tonemapping" => v.tone_mapping = Some(val.to_owned()),
                "colorizationset" => v.colorization_set = Some(val.to_owned()),
                "r_hdrskyintensity" => v.hdr_sky_intensity = one(),
                "r_litsurfacehdrscalar" => v.lit_surface_scalar = one(),
                "r_unlitsurfacehdrscalar" => v.unlit_surface_scalar = one(),
                "r_filmlighttint" => light = three(),
                "r_filmmediumtint" => medium = three(),
                "r_filmdarktint" => dark = three(),
                "r_filmdesaturation" => v.desaturation = one(),
                "r_glowbloomcutoff" => v.bloom_cutoff = one(),
                "r_glowbloomintensity0" => v.bloom_intensity = one(),
                "r_materialbloomintensity" => v.material_bloom_intensity = one(),
                _ => {}
            }
        }
        if let (Some(l), Some(m), Some(d)) = (light, medium, dark) {
            v.film_tints = Some([l, m, d]);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag() -> Hable {
        let mut v = [0f32; 26];
        let groups: [[f32; 3]; 8] = [[0.8; 3], [-2.0; 3], [10.0; 3], [3.0; 3], [1.0; 3], [1.0; 3], [16.0; 3], [0.4, 2.2, 1.1]];
        for (i, g) in groups.iter().enumerate() {
            v[3 * i..3 * i + 3].copy_from_slice(g);
        }
        v[24] = 1.0;
        v[25] = 1.0;
        Hable::from_asset(&ToneMappingRec { name: "mp_frag_tone".into(), v }).unwrap()
    }

    #[test]
    fn the_curve_starts_at_zero_rises_and_ends_at_the_white_point() {
        let h = frag();
        assert!(h.curve(0.0).abs() < 1e-6);
        let mut last = -1.0;
        for i in 0..400 {
            let d = h.display_at(i as f32 * 0.05, 1.0);
            assert!(d >= last - 1e-6);
            last = d;
        }
        assert!((h.display_at(16.0, 1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn mp_frags_numbers_put_a_grey_in_its_sun_at_the_middle_of_the_band() {
        // Sun 8 (the ComWorld's peak) over a baked shaded level of 0.432.
        let g = frag().display(GREY * (8.0 + 0.432));
        assert!((g - 0.65).abs() < 0.02, "{g}");
    }

    #[test]
    fn solving_for_the_branch_reproduces_both_targets() {
        let h = frag();
        let s = solve(&h, 8.0, 0.432, 0.4).expect("a solution");
        let lit = super::super::bo3_look::tone(GREY * (s.sun_light + 0.4), s.exposure);
        let shade = super::super::bo3_look::tone(GREY * 0.4, s.exposure);
        if !s.clamped {
            assert!((lit - s.grey_lit.clamp(0.5, 0.85)).abs() < 0.01, "{lit}");
        }
        assert!((shade - s.grey_shade.clamp(0.14, 0.55)).abs() < 0.01, "{shade}");
        assert!(s.sun_light >= 1.07 * 0.4 - 1e-4 && s.sun_light <= 3.55 * 0.4 + 1e-4);
    }

    #[test]
    fn a_zone_without_the_asset_takes_hables_defaults_at_a_solved_exposure() {
        let h = curve_for(&Vision::default(), &HashMap::new(), 8.4, 0.65);
        assert_eq!((h.a, h.white), (0.15, 11.2));
        assert!((h.display(GREY * 8.4) - 0.65).abs() < 0.01);
        assert!(h.source.contains("defaults"));
    }

    #[test]
    fn the_exposure_floats_are_a_range_and_adaptation_follows_the_rates() {
        let h = frag();
        assert_eq!((h.exposure_min, h.exposure_max, h.exposure_third), (0.4, 2.2, 1.1));
        assert!(h.adaptive());
        assert!((h.settled_exposure(10.0, 0.18) - 0.4).abs() < 1e-6, "a bright scene sits at the floor");
        assert!((h.settled_exposure(0.01, 0.18) - 2.2).abs() < 1e-6, "a dark one at the ceiling");
        let mut still = h.clone();
        still.rates = [0.0, 1.0];
        assert!(!still.adaptive());
        assert!((still.held_exposure() - 1.3).abs() < 1e-6);
        assert!((still.settled_exposure(10.0, 0.18) - 1.3).abs() < 1e-6);
    }

    /// Prison Break's day asset (`mp_prisonbreak_tone_01`): the linear
    /// strength is 1.6 in red and green and 2.6 in blue.
    fn prison_break() -> ToneMappingRec {
        let mut v = [0f32; 26];
        let groups: [[f32; 3]; 8] = [[2.2; 3], [0.2; 3], [2.0; 3], [1.6, 1.6, 2.6], [1.6; 3], [5.0; 3], [11.0; 3], [0.21, 0.34, 0.13]];
        for (i, g) in groups.iter().enumerate() {
            v[3 * i..3 * i + 3].copy_from_slice(g);
        }
        v[24] = 3.0;
        v[25] = 3.0;
        ToneMappingRec { name: "mp_prisonbreak_tone_01".into(), v }
    }

    #[test]
    fn each_channel_keeps_its_own_parameters() {
        let t = prison_break();
        let lum = Hable::from_asset(&t).unwrap();
        let [r, g, b] = [0, 1, 2].map(|c| Hable::from_asset_channel(&t, c).unwrap());
        assert_eq!((r.b, g.b, b.b), (1.6, 1.6, 2.6));
        assert!((lum.b - (0.9278 * 1.6 + 0.0722 * 2.6)).abs() < 1e-4);
        assert!(r.from_zone && b.source.contains("blue"));
        assert!(!Hable::published().from_zone);
    }

    #[test]
    fn the_inverse_takes_a_display_value_back_to_its_light() {
        let h = frag();
        for x in [0.01f32, 0.2, 1.0, 3.0, 9.0] {
            let d = h.display_at(x, h.exposure);
            let back = h.inverse_display(d, h.exposure);
            assert!((h.display_at(back, h.exposure) - d).abs() < 1e-4, "{x}: {back}");
        }
        assert_eq!(h.inverse_display(0.0, 1.0), 0.0);
        assert!((h.inverse_display(1.0, 2.0) - 8.0).abs() < 1e-6);
    }

    #[test]
    fn the_channel_table_is_the_identity_when_every_channel_is_alike() {
        let h = frag();
        let mut t = ToneMappingRec { name: "mp_frag_tone".into(), v: [0.0; 26] };
        let groups: [[f32; 3]; 8] = [[0.8; 3], [-2.0; 3], [10.0; 3], [3.0; 3], [1.0; 3], [1.0; 3], [16.0; 3], [0.4, 2.2, 1.1]];
        for (i, g) in groups.iter().enumerate() {
            t.v[3 * i..3 * i + 3].copy_from_slice(g);
        }
        let ch = [0, 1, 2].map(|c| Hable::from_asset_channel(&t, c).unwrap());
        let lut = channel_lut(&h, &ch, h.exposure, None);
        assert_eq!(lut.len(), LUT * LUT * LUT * 4);
        for (i, px) in lut.chunks_exact(4).enumerate() {
            let want = [i % LUT, (i / LUT) % LUT, i / (LUT * LUT)].map(|k| (k as f32 / (LUT - 1) as f32 * 255.0).round() as i32);
            for c in 0..3 {
                assert!((i32::from(px[c]) - want[c]).abs() <= 1, "entry {i}: {px:?} against {want:?}");
            }
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn prison_breaks_blue_curve_moves_the_blue_channel_only() {
        let t = prison_break();
        let lum = Hable::from_asset(&t).unwrap();
        let ch = [0, 1, 2].map(|c| Hable::from_asset_channel(&t, c).unwrap());
        let s = channel_spread(&lum, &ch, lum.exposure);
        assert!((s[0] - s[1]).abs() < 1e-6, "{s:?}");
        assert!((s[2] - s[0]).abs() > 0.005, "{s:?}");
        let lut = channel_lut(&lum, &ch, lum.exposure, None);
        // The middle entry: red and green near where they went in.
        let mid = LUT / 2;
        let at = 4 * (mid + LUT * mid + LUT * LUT * mid);
        let d = (mid as f32 / (LUT - 1) as f32 * 255.0).round() as i32;
        assert!((i32::from(lut[at]) - d).abs() <= 6, "{}", lut[at]);
        assert_ne!(lut[at + 2], lut[at]);
    }

    #[test]
    fn the_iris_settles_inside_the_assets_range_and_leaves_the_defaults_alone() {
        let h = frag();
        // mp_frag's sunlit grey (sun 8 over shade 0.43): bright, so the floor.
        assert!((h.iris_exposure(0.35 * 8.43) - 0.4).abs() < 1e-6);
        assert!((h.iris_exposure(0.05) - 2.2).abs() < 1e-6);
        let p = Hable::published().with_solved_exposure(8.4, 0.65);
        assert_eq!(p.iris_exposure(0.05), p.exposure);
        if std::env::var_os("BO2MP_IW6_IRIS").is_none() && std::env::var_os("BO2MP_IW6_TONE_RGB").is_none() {
            assert!(iris_on() && rgb_on());
        }
    }

    #[test]
    fn the_iris_settles_where_ghosts_iris_steps_come_to_rest() {
        // `fastfile_iw6::shading::iris_step` frame after frame (exposure (min,
        // max, key), rates (down, up, dt)) ends where `iris_exposure` says.
        let t = prison_break();
        for h in [frag(), Hable::from_asset(&t).unwrap()] {
            for lum in [0.02f32, 0.1, 0.4, 3.0] {
                let exposure = [h.exposure_min, h.exposure_max, h.exposure_third, 0.0];
                let rates = [h.rates[0], h.rates[1], 1.0 / 60.0, 0.0];
                let mut e = h.exposure_min;
                for _ in 0..20_000 {
                    e = fastfile_iw6::shading::iris_step(e, lum, exposure, rates);
                }
                assert!((e - h.iris_exposure(lum)).abs() < 1e-3, "{}: lum {lum}: {e} against {}", h.source, h.iris_exposure(lum));
            }
        }
    }

    #[test]
    fn a_broken_asset_is_refused() {
        let mut v = [1f32; 26];
        v[18..21].fill(0.0);
        assert!(Hable::from_asset(&ToneMappingRec { name: "x".into(), v }).is_none());
        v[18] = f32::NAN;
        assert!(Hable::from_asset(&ToneMappingRec { name: "x".into(), v }).is_none());
    }

    #[test]
    fn the_vision_text_is_read() {
        let text = "r_filmLightTint \"1.075 1.067 1.051\"\nr_filmMediumTint \"1.043 1.059 1.075\"\nr_filmDarkTint \"1.216 1.32 1.51\"\nr_hdrSkyIntensity \"3\"\nr_litSurfaceHDRScalar \"0.2\"\ncolorizationSet\t\"mp_frag_colorset\"\ntoneMapping\t\"mp_frag_tone\"\nr_glowBloomCutoff \"0.85\"\n";
        let v = Vision::parse(text);
        assert_eq!(v.tone_mapping.as_deref(), Some("mp_frag_tone"));
        assert_eq!(v.colorization_set.as_deref(), Some("mp_frag_colorset"));
        assert_eq!(v.hdr_sky_intensity, Some(3.0));
        assert_eq!(v.lit_surface_scalar, Some(0.2));
        assert_eq!(v.film_tints.unwrap()[2], [1.216, 1.32, 1.51]);
        assert_eq!(v.bloom_cutoff, Some(0.85));
    }

    #[test]
    fn the_mode_follows_the_switch_value() {
        // (The environment is shared between tests: only the default is checked.)
        if std::env::var_os("BO2MP_IW6_TONEMAP").is_none() {
            assert_eq!(mode(Mode::Bo3), Mode::Bo3);
            assert_eq!(mode(Mode::Hable), Mode::Hable);
        }
    }
}
