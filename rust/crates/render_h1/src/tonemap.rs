//! MWR's own tone map maths, read from the game executable's code.
//!
//! Function level only (no addresses or code are kept); what was read and
//! what was inferred is listed in `docs/H1_NATIVE.md`.
//!
//! What the executable does, in short:
//!
//! * The frame's exposure (a multiplier on the HDR scene) is either
//!   automatic, `key / average`, the average being the log-average
//!   luminance of the scene, or a fixed EV: `0.18 * 2^(2.84 - EV) / b`.
//!   `b` is a view scalar (0.001 when unset). Then `tonemapexposureadjust`
//!   stops are added and the exposure is capped at the EV of
//!   `tonemapmaxexposure`.
//! * The key is `0.18 * 2^tonemapautoexposureadjust` while the light set
//!   locks the adjust (the default), else a curve through the dark, middle
//!   and light EV points ([`AdjustCurve`]).
//! * The filmic curve (light set `tonemap` 2) is two rational segments built
//!   from the light set's black, white, crossover, shoulder and toe
//!   ([`Filmic`]); the exposure is folded into their coefficients.

use crate::look::Settings;

/// Which tone map maths the host uses (`BO2MP_H1_TONEMAP`).
///
/// * `Fitted`: the curve fitted to HorizonMW's picture of Rust
///   ([`crate::look::filmic_buffer`]).
/// * `Exact` (the default since 30 Oct, the owner's decision: the game's
///   own maths): this module.
///
/// The fitted numbers (0.70 at EV 10) do not equal the executable's
/// exposure for EV 10 (1.26): the fit was made against Rust's linear tone
/// map. See `docs/H1_NATIVE.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneMath {
    Fitted,
    Exact,
}

impl ToneMath {
    pub fn parse(v: &str) -> Self {
        if v.trim().eq_ignore_ascii_case("fitted") {
            Self::Fitted
        } else {
            Self::Exact
        }
    }

    pub fn from_env() -> Self {
        std::env::var("BO2MP_H1_TONEMAP").map_or(Self::Exact, |v| Self::parse(&v))
    }
}

/// The filmic buffer by the chosen maths. `scale` is the host's exposure
/// (`Look::exposure`). With the exact maths it is the engine's exposure
/// itself (the auto-exposure result, or the stops the host adds); a light
/// set with automatic exposure off takes its own EV on top.
/// `metered`: `scale` is already the engine's whole exposure (the meter's
/// automatic or fixed exposure with the adjust and the cap), as in game.
pub fn filmic_buffer_for(math: ToneMath, lightset: &Settings, scale: f32, metered: bool) -> Vec<[f32; 4]> {
    match math {
        ToneMath::Fitted => crate::look::filmic_buffer(lightset, scale),
        ToneMath::Exact => filmic_buffer(lightset, scale, metered),
    }
}

/// What the filmic curve's input is per unit of exposure. Read from
/// `code_post_gfx_mp`'s `cs_logaccumfinish` (9 Nov 2026): the exposure pass
/// scales the curve's rows by `exposure * 5.55556` (`1 / 0.18`) and divides
/// the crossover by it, so `tonemap_filmic` sees `c = x E / 0.18`; an
/// automatic exposure (`E = 0.18 / L`) puts the scene's log-average at
/// `c = 1`. (`tonemap_linear` takes `E` itself.) Before, we fed `c = x E`,
/// 2.47 stops dark, made up for by the fitted [`auto_key_boost`].
pub const FILMIC_GAIN: f32 = 1.0 / GREY;

/// `BO2MP_H1_FILMIC_EXPOSURE=old`: the behaviour before the read of
/// `cs_logaccumfinish` (no [`FILMIC_GAIN`], the fitted key boost, the fixed
/// light sets' black point capped, a metered fixed exposure counted twice).
pub fn old_filmic() -> bool {
    std::env::var("BO2MP_H1_FILMIC_EXPOSURE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("old"))
}

/// Mid grey: the grey a scene's average luminance is brought to
/// (`0.18 * 2^adjust`).
pub const GREY: f32 = 0.18;
/// The EV that a scene luminance of `b` (the HDR scalar) has.
pub const EV_BIAS: f32 = 2.84;
/// The view's HDR scalar when it is unset (zero or less).
pub const HDR_SCALAR_DEFAULT: f32 = 0.001;
/// The gamma the adjust curve is interpolated in.
pub const ADJUST_GAMMA: f32 = 2.2;
/// A scene luminance below this is not measured (the exposure holds).
pub const MIN_LUMINANCE: f32 = 6.1e-5;

fn scalar(b: f32) -> f32 {
    if b > 0.0 { b } else { HDR_SCALAR_DEFAULT }
}

/// The exposure (multiplier on the scene) of a fixed EV:
/// `0.18 * 2^(2.84 - ev) / b`. One stop per EV.
pub fn ev_to_exposure(ev: f32, b: f32) -> f32 {
    GREY / (2f32.powf(ev - EV_BIAS) * scalar(b))
}

/// The EV of an exposure; the inverse of [`ev_to_exposure`].
pub fn exposure_to_ev(exposure: f32, b: f32) -> f32 {
    (GREY / exposure / scalar(b) + 6.1e-5).log2() + EV_BIAS
}

/// `0.18 * 2^adjust`: the level an exposure adjust (in stops) names.
pub fn grey_of_adjust(adjust: f32) -> f32 {
    GREY * 2f32.powf(adjust)
}

/// The display level (linear, after the filmic curve) the automatic exposure
/// aims the scene's log-average at. INFERRED, fitted to the retail maps' look
/// (their fixed exposure puts a middle pixel near 0.1; usermaps measured too bright at 0.10) and to a usermap
/// screenshot from the game: with the exe's key of 0.18 going into the curve,
/// a usermap's light set (crossover 2.5-3, black 0.05) shows it as 0.014, so
/// the whole frame came out about three stops dark.
/// `BO2MP_H1_AUTO_KEY=<level>` sets it, `exe` keeps the key as read.
pub const AUTO_DISPLAY_LEVEL: f32 = 0.07;

impl Filmic {
    /// The scene value (times exposure) the curve shows as `y`; the inverse
    /// of [`Filmic::eval`], on the toe below the crossover level, else the
    /// shoulder.
    pub fn inverse(&self, y: f32) -> f32 {
        let (t, s) = self.rows();
        let r = if y <= self.crossover_level() { t } else { s };
        let den = r[0] - r[1] * y;
        if den.abs() < 1e-9 {
            return self.crossover;
        }
        ((r[3] * y - r[2]) / den).max(0.0)
    }
}

/// How much the display-level aim lifts the exe's automatic key
/// (`auto_key`, which assumes 0.18 is a middle grey) for this light set's
/// curve; 1 when the switch says `exe` or the map's tone map is not filmic.
pub fn auto_key_boost(ls: &Settings) -> f32 {
    let level = match std::env::var("BO2MP_H1_AUTO_KEY") {
        Ok(v) if v.trim().eq_ignore_ascii_case("exe") => return 1.0,
        Ok(v) => v.trim().parse::<f32>().unwrap_or(AUTO_DISPLAY_LEVEL),
        Err(_) => AUTO_DISPLAY_LEVEL,
    };
    if ls.f("tonemap", 0.0) != 2.0 {
        return 1.0;
    }
    let c = Filmic::from_lightset(ls).inverse(level.clamp(0.01, 0.9));
    let boost = if c.is_finite() { (c / GREY).max(1.0) } else { 1.0 };
    // (The light set's own lift counts toward it: our boost times its
    // `tonemapautoexposureadjust` and `tonemapexposureadjust` stops never
    // passes [`AUTO_TOTAL_CAP`].)
    let own = 2f32.powf(
        ls.f("tonemapautoexposureadjust", 0.0).max(0.0) + ls.f("tonemapexposureadjust", 0.0).max(0.0),
    );
    boost.min((AUTO_TOTAL_CAP / own).max(1.0))
}

/// The most the automatic key is lifted in all (our boost times the light
/// set's own adjust). INFERRED from the 64 Horizon MW usermaps: their
/// lifts are 3-8 except Highrise, whose hand-made curve (crossover level
/// 0.011) and own +3.26 stops made 245 and a white frame; with the exe's
/// key alone it matched its loading picture (0.91).
pub const AUTO_TOTAL_CAP: f32 = 8.0;

/// [`auto_key`] as the frame uses it: lifted to the display aim.
/// Only with [`old_filmic`]: the engine has no boost; the read gain
/// ([`FILMIC_GAIN`]) is what the fitted boost stood in for.
pub fn shown_key(ls: &Settings, lum: f32, b: f32) -> f32 {
    auto_key(ls, lum, b) * key_boost_in_use(ls)
}

/// [`auto_key_boost`] when [`old_filmic`], else 1.
pub fn key_boost_in_use(ls: &Settings) -> f32 {
    if old_filmic() { auto_key_boost(ls) } else { 1.0 }
}

/// The automatic exposure's target curve: the light set's three EV points
/// with the adjust (stops) wanted at each, and the clamp. Defaults are the
/// dvars' (`r_tonemapDarkEv` ...).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdjustCurve {
    pub dark_ev: f32,
    pub mid_ev: f32,
    pub light_ev: f32,
    pub dark_adjust: f32,
    pub mid_adjust: f32,
    pub light_adjust: f32,
    pub min_adjust: f32,
    pub max_adjust: f32,
}

impl Default for AdjustCurve {
    fn default() -> Self {
        Self {
            dark_ev: 2.84,
            mid_ev: 7.823,
            light_ev: 12.81,
            dark_adjust: -3.17,
            mid_adjust: -0.0651,
            light_adjust: 1.47,
            min_adjust: -3.17,
            max_adjust: 2.3,
        }
    }
}

impl AdjustCurve {
    pub fn from_lightset(ls: &Settings) -> Self {
        let d = Self::default();
        Self {
            dark_ev: ls.f("tonemapdarkev", d.dark_ev),
            mid_ev: ls.f("tonemapmidev", d.mid_ev),
            light_ev: ls.f("tonemaplightev", d.light_ev),
            dark_adjust: ls.f("tonemapdarkexposureadjust", d.dark_adjust),
            mid_adjust: ls.f("tonemapmidexposureadjust", d.mid_adjust),
            light_adjust: ls.f("tonemaplightexposureadjust", d.light_adjust),
            min_adjust: ls.f("tonemapminexposureadjust", d.min_adjust),
            max_adjust: ls.f("tonemapmaxexposureadjust", d.max_adjust),
        }
    }

    /// The grey level the scene's average luminance `lum` is brought to.
    /// The luminance is read as `ln(lum / b)` on an axis whose points are
    /// `ln 2^(ev - 2.84)`; between the dark and light points a power curve
    /// through the mid point is laid on the gamma-2.2 greys
    /// (`0.18 * 2^adjust`), and the result is clamped to the greys of the
    /// min and max adjust.
    pub fn target(&self, lum: f32, b: f32) -> f32 {
        let ln2 = std::f32::consts::LN_2;
        let axis = |ev: f32| (ev - EV_BIAS) * ln2;
        let (xd, xm, xl) = (axis(self.dark_ev), axis(self.mid_ev), axis(self.light_ev));
        let x = (lum.max(1e-12) / scalar(b)).ln();
        let t = ((x - xd) / (xl - xd)).max(0.0);
        let g = |adj: f32| grey_of_adjust(adj).powf(1.0 / ADJUST_GAMMA);
        let (g0, g1, g2) = (
            g(self.dark_adjust),
            g(self.mid_adjust),
            g(self.light_adjust),
        );
        let tm = (xm - xd) / (xl - xd);
        let k = ((g1 - g0) / (g2 - g0)).ln() / tm.ln();
        let u = if t > 0.0 { t.powf(k) } else { 0.0 };
        let v = (u * (g2 - g0) + g0).max(0.0).powf(ADJUST_GAMMA);
        let (lo, hi) = (
            grey_of_adjust(self.min_adjust),
            grey_of_adjust(self.max_adjust),
        );
        v.min(hi).max(lo)
    }
}

/// The grey the automatic exposure aims the scene's average at:
/// `0.18 * 2^autoexposureadjust` when the light set locks the adjust
/// (`tonemaplockautoexposureadjust`, the default), else the curve.
pub fn auto_key(ls: &Settings, lum: f32, b: f32) -> f32 {
    if ls.f("tonemaplockautoexposureadjust", 1.0) != 0.0 {
        grey_of_adjust(ls.f("tonemapautoexposureadjust", 0.0))
    } else {
        AdjustCurve::from_lightset(ls).target(lum, b)
    }
}

/// Whether the frame's exposure is automatic: the light set says so, or
/// `BO2MP_H1_FIXED_AS_AUTO=1` treats a fixed-exposure set that way (test).
pub fn is_auto(ls: &Settings) -> bool {
    ls.f("tonemapauto", 1.0) != 0.0 || std::env::var("BO2MP_H1_FIXED_AS_AUTO").is_ok_and(|v| v == "1")
}

/// The exposure of the frame: automatic (`key / average`, the average being
/// the log-average luminance of the HDR scene) or a fixed EV; then the light
/// set's `tonemapexposureadjust` stops added, and the cap of
/// `tonemapmaxexposure` (an EV: the exposure never exceeds that EV's).
pub fn exposure(ls: &Settings, lum: f32, b: f32) -> f32 {
    let auto = is_auto(ls);
    let e = if auto {
        shown_key(ls, lum, b) / lum.max(MIN_LUMINANCE)
    } else {
        ev_to_exposure(ls.f("tonemapexposure", 10.0), b)
    };
    let e = e * 2f32.powf(ls.f("tonemapexposureadjust", 0.0));
    e.min(ev_to_exposure(ls.f("tonemapmaxexposure", -10.0), b))
}

/// One adaptation step of ln(exposure), as the frame code does when
/// `tonemapblend` is on: toward the target by `speed` (the light set's
/// `tonemapadaptspeed`, 0.02) per frame, held when the scene is too dark to
/// measure.
pub fn adapt(prev_ln_exposure: f32, target_exposure: f32, lum: f32, speed: f32) -> f32 {
    let target = target_exposure.max(MIN_LUMINANCE).ln();
    if lum > MIN_LUMINANCE {
        (target - prev_ln_exposure) * speed + prev_ln_exposure
    } else {
        prev_ln_exposure
    }
}

/// The filmic curve's two rational segments: `y = (a c + b) / (cc c + d)`
/// below the crossover (the toe) and with the other set above it (the
/// shoulder), `c` being the scene times the exposure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filmic {
    pub black: f32,
    pub white: f32,
    pub crossover: f32,
    pub shoulder: f32,
    pub toe: f32,
}

impl Default for Filmic {
    /// The dvars' defaults (`r_tonemapBlack` ...), taken when a light set
    /// does not say.
    fn default() -> Self {
        Self {
            black: 0.0,
            white: 512.0,
            crossover: 2.0,
            shoulder: 0.95,
            toe: 0.2,
        }
    }
}

/// `[a, cc, b, d]`, the order the shader's rows hold them.
pub type Row = [f32; 4];

impl Filmic {
    pub fn from_lightset(ls: &Settings) -> Self {
        let d = Self::default();
        Self {
            black: Self::black_of(ls, d.black),
            white: ls.f("tonemapwhite", d.white),
            crossover: ls.f("tonemapcrossover", d.crossover),
            shoulder: ls.f("tonemapshoulder", d.shoulder),
            toe: ls.f("tonemaptoe", d.toe),
        }
    }

    /// The curve's black point. INFERRED: a retail light set with a fixed
    /// exposure and a black point of 0.2 (Rundown) puts the scene's own
    /// log-average (0.1 before the exposure) under the black point, so the
    /// shade and most of the frame clip to black while every other fixed
    /// exposure map averages 0.16-0.36 on screen. Those maps keep only a
    /// small black point. `BO2MP_H1_BLACK=exe` takes the value as stored.
    fn black_of(ls: &Settings, default: f32) -> f32 {
        let black = ls.f("tonemapblack", default);
        // (9 Nov 2026: with [`FILMIC_GAIN`] read, the frame is no longer
        // 2.5 stops under the black point; the cap stays only with
        // [`old_filmic`].)
        let fixed = ls.f("tonemapauto", 0.0) == 0.0;
        let exact = std::env::var("BO2MP_H1_BLACK").is_ok_and(|v| v.trim().eq_ignore_ascii_case("exe"));
        if fixed && !exact && old_filmic() {
            black.min(0.02)
        } else {
            black
        }
    }

    /// The output level at the crossover: with `f = shoulder^0.25` and
    /// `e2 = toe^2`, `n = (s2 - s0)(1 - e2)`,
    /// `q = n / ((s1 - s2)(1 - f) + n)`.
    pub fn crossover_level(&self) -> f32 {
        let f = self.shoulder.powf(0.25);
        let e2 = self.toe * self.toe;
        let n = (self.crossover - self.black) * (1.0 - e2);
        n / ((self.white - self.crossover) * (1.0 - f) + n)
    }

    /// (toe row, shoulder row) for input `c` (scene times exposure).
    pub fn rows(&self) -> (Row, Row) {
        let (s0, s1, s2) = (self.black, self.white, self.crossover);
        let f = self.shoulder.powf(0.25);
        let e2 = self.toe * self.toe;
        let q = self.crossover_level();
        let toe = [q - q * e2, -e2, q * s0 * (e2 - 1.0), s2 - (1.0 - e2) * s0];
        let shoulder = [
            q * f - q + 1.0,
            f,
            q * s1 * (1.0 - f) - s2,
            (1.0 - f) * s1 - s2,
        ];
        (toe, shoulder)
    }

    /// The curve on `c` (no exposure, no gamma).
    pub fn eval(&self, c: f32) -> f32 {
        let (t, s) = self.rows();
        let r = if c <= self.crossover { t } else { s };
        (r[0] * c + r[2]) / (r[1] * c + r[3])
    }

    /// The `tonemap_filmic` buffer for scene value `x` with `c = exposure x`
    /// folded in: `a` and `cc` times the exposure, the crossover over it;
    /// `[3].z` is 1 / gamma (`r_tonemapGamma`, 1).
    pub fn buffer(&self, exposure: f32, gamma: f32) -> Vec<[f32; 4]> {
        let (t, s) = self.rows();
        let fold = |r: Row| [r[0] * exposure, r[1] * exposure, r[2], r[3]];
        vec![
            [self.crossover / exposure, 0.0, 0.0, 0.0],
            fold(s),
            fold(t),
            [0.0, 0.0, 1.0 / gamma.max(1e-3), 0.0],
        ]
    }
}

/// The filmic buffer of a light set at the host's exposure `scale`
/// (`metered`: the engine's whole exposure, so nothing more is added).
pub fn filmic_buffer(ls: &Settings, scale: f32, metered: bool) -> Vec<[f32; 4]> {
    let old = old_filmic();
    let e = if metered && !old {
        scale
    } else {
        let adjust = 2f32.powf(ls.f("tonemapexposureadjust", 0.0));
        if !is_auto(ls) {
            ev_to_exposure(ls.f("tonemapexposure", 10.0), 0.0) * adjust * scale
        } else {
            scale * adjust
        }
    };
    let gain = if old { 1.0 } else { FILMIC_GAIN };
    Filmic::from_lightset(ls).buffer(e * gain, 1.0)
}

/// The per-map "h1 vision:" line: what the light set and vision say and what
/// the exact maths makes of it (`lum` the scene's log-average luminance, if
/// measured).
pub fn vision_line(map: &str, ls: &Settings, vision: &Settings, lum: Option<f32>) -> String {
    let mode = match ls.f("tonemap", 3.0) as i32 {
        0 => "perceptual",
        1 => "reinhard",
        2 => "filmic",
        3 => "linear",
        4 => "luminance",
        _ => "unknown",
    };
    let f = Filmic::from_lightset(ls);
    format!(
        "h1 vision: {map}: tone map {mode}, {} (EV {:.2}, adjust {:+.2}, lock {}), \
         curve black {} white {} crossover {} shoulder {} toe {} (crossover level {:.3}), \
         exposure {}; film {}",
        if is_auto(ls) {
            "auto"
        } else {
            "fixed"
        },
        ls.f("tonemapexposure", 10.0),
        ls.f("tonemapexposureadjust", 0.0),
        ls.f("tonemaplockautoexposureadjust", 1.0) != 0.0,
        f.black,
        f.white,
        f.crossover,
        f.shoulder,
        f.toe,
        f.crossover_level(),
        lum.map_or_else(
            || "at the measured average".to_owned(),
            |l| format!("x{:.3}", exposure(ls, l, 0.0))
        ),
        if vision.f("r_filmEnable", 0.0) != 0.0 {
            "on"
        } else {
            "off"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_set_that_lifts_itself_is_not_boosted_past_the_cap() {
        // highrise: its own +3.26 stops already pass the cap
        let hr = with(&[
            ("tonemap", 2.0),
            ("tonemapexposureadjust", 3.26125),
            ("tonemapblack", 0.1),
            ("tonemapwhite", 1592.54),
            ("tonemapcrossover", 0.830135),
            ("tonemapshoulder", 0.858745),
            ("tonemaptoe", 0.333319),
        ]);
        assert_eq!(auto_key_boost(&hr), 1.0);
        // favela's generic curve keeps its boost (about 3.9)
        let fv = with(&[("tonemap", 2.0), ("tonemapblack", 0.05), ("tonemapcrossover", 3.0), ("tonemapshoulder", 0.95)]);
        let b = auto_key_boost(&fv);
        assert!(b > 3.5 && b < 4.5, "{b}");
    }

    #[test]
    fn the_curve_inverse_round_trips_and_the_boost_lifts_a_usermap_key() {
        let f = Filmic { black: 0.05, white: 451.0, crossover: 2.5, shoulder: 0.94, toe: 0.0 };
        for y in [0.01f32, 0.05, 0.1, 0.2, 0.26, 0.5, 0.8] {
            let c = f.inverse(y);
            assert!((f.eval(c) - y).abs() < 2e-3, "{y}: {c} -> {}", f.eval(c));
        }
        let mut ls = Settings::default();
        ls.values.insert("tonemap".into(), vec![2.0]);
        ls.values.insert("tonemapblack".into(), vec![0.05]);
        ls.values.insert("tonemapcrossover".into(), vec![2.5]);
        ls.values.insert("tonemapshoulder".into(), vec![0.94]);
        ls.values.insert("tonemaptoe".into(), vec![0.0]);
        let b = auto_key_boost(&ls);
        assert!(b > 3.0 && b < 12.0, "{b}");
        let none = Settings::default();
        assert_eq!(auto_key_boost(&none), 1.0);
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    fn with(pairs: &[(&str, f32)]) -> Settings {
        let mut ls = Settings::default();
        for (k, v) in pairs {
            ls.values.insert((*k).to_owned(), vec![*v]);
        }
        ls
    }

    #[test]
    fn exposure_is_one_stop_per_ev() {
        let e10 = ev_to_exposure(10.0, 0.0);
        assert!(close(e10, 1.2586, 1e-3), "{e10}");
        assert!(close(ev_to_exposure(11.0, 0.0), e10 / 2.0, 1e-5));
        assert!(close(exposure_to_ev(e10, 0.0), 10.0, 1e-3));
        // An unset or non-positive scalar is the 0.001 default.
        assert_eq!(ev_to_exposure(7.0, -1.0), ev_to_exposure(7.0, 0.001));
    }

    #[test]
    fn filmic_hits_black_white_and_is_continuous() {
        let f = Filmic::default();
        assert!(f.eval(f.black).abs() < 1e-6);
        assert!(close(f.eval(f.white), 1.0, 1e-5));
        assert!(close(f.eval(f.crossover), f.crossover_level(), 1e-5));
        let (t, s) = f.rows();
        let at = |r: Row, c: f32| (r[0] * c + r[2]) / (r[1] * c + r[3]);
        assert!(close(at(t, f.crossover), at(s, f.crossover), 1e-5));
        let mut prev = -1.0;
        for i in 0..2000 {
            let y = f.eval(i as f32 * 0.3);
            assert!(y >= prev - 1e-6, "{i}");
            prev = y;
        }
    }

    #[test]
    fn filmic_with_a_black_point_maps_it_to_zero() {
        let f = Filmic {
            black: 0.01,
            ..Filmic::default()
        };
        assert!(f.eval(0.01).abs() < 1e-6);
        assert!(close(f.eval(f.white), 1.0, 1e-5));
    }

    #[test]
    fn filmic_buffer_folds_the_exposure() {
        let ls = with(&[("tonemapwhite", 300.0), ("tonemapcrossover", 1.5)]);
        let f = Filmic::from_lightset(&ls);
        let e = 0.7;
        let b = f.buffer(e, 1.0);
        for x in [0.01f32, 0.3, 1.0, 2.0, 5.0, 100.0] {
            let row = if x <= b[0][0] { b[2] } else { b[1] };
            let y = (x * row[0] + row[2]) / (x * row[1] + row[3]);
            assert!(close(y, f.eval(x * e), 1e-4), "{x}: {y} {}", f.eval(x * e));
        }
        assert_eq!(b[3][2], 1.0);
    }

    #[test]
    fn a_metered_exposure_goes_in_once_with_the_read_gain() {
        // cs_logaccumfinish: rows times E / 0.18, crossover over it.
        let ls = with(&[("tonemap", 2.0), ("tonemapauto", 0.0), ("tonemapexposure", 8.8), ("tonemapexposureadjust", 1.0)]);
        let e = 2.5;
        let b = filmic_buffer(&ls, e, true);
        let f = Filmic::from_lightset(&ls);
        assert!(close(b[0][0], f.crossover / (e * FILMIC_GAIN), 1e-5));
        let (_, s) = f.rows();
        assert!(close(b[1][0], s[0] * e * FILMIC_GAIN, 1e-5));
        // An automatic exposure of 0.18 / L puts L at c = 1.
        let lum = 0.4;
        assert!(close(GREY / lum * FILMIC_GAIN * lum, 1.0, 1e-6));
    }

    #[test]
    fn adjust_curve_passes_through_its_points() {
        let c = AdjustCurve::default();
        let b = 0.001;
        // The luminance of an EV: b * 2^(ev - 2.84).
        let lum = |ev: f32| b * 2f32.powf(ev - EV_BIAS);
        assert!(close(
            c.target(lum(c.dark_ev), b),
            grey_of_adjust(c.dark_adjust),
            1e-3
        ));
        assert!(close(
            c.target(lum(c.mid_ev), b),
            grey_of_adjust(c.mid_adjust),
            1e-2
        ));
        assert!(close(
            c.target(lum(c.light_ev), b),
            grey_of_adjust(c.light_adjust),
            1e-2
        ));
        assert!(close(c.target(1e-9, b), grey_of_adjust(c.min_adjust), 1e-4));
        assert!(close(c.target(1e6, b), grey_of_adjust(c.max_adjust), 1e-4));
    }

    #[test]
    fn locked_adjust_is_the_plain_grey() {
        // The default light set: locked, adjust 0: the average is brought to
        // 0.18, the key the host already used.
        let ls = Settings::default();
        assert!(close(auto_key(&ls, 0.37, 0.0), 0.18, 1e-6));
        assert!(close(exposure(&ls, 0.36, 0.0), 0.5, 1e-5));
        let ls = with(&[("tonemapexposureadjust", 1.0)]);
        assert!(close(exposure(&ls, 0.36, 0.0), 1.0, 1e-5));
        // Unlocked: the curve.
        let ls = with(&[("tonemaplockautoexposureadjust", 0.0)]);
        let k = auto_key(&ls, 0.0316, 0.001);
        assert!(close(k, grey_of_adjust(-0.0651), 2e-2), "{k}");
    }

    #[test]
    fn fixed_ev_exposure_and_cap() {
        let ls = with(&[("tonemapauto", 0.0), ("tonemapexposure", 10.0)]);
        assert!(close(exposure(&ls, 1.0, 0.0), 1.2586, 1e-3));
        let ls = with(&[
            ("tonemapauto", 0.0),
            ("tonemapexposure", 10.0),
            ("tonemapmaxexposure", 12.0),
        ]);
        assert!(close(
            exposure(&ls, 1.0, 0.0),
            ev_to_exposure(12.0, 0.0),
            1e-5
        ));
    }

    #[test]
    fn adaptation_moves_in_the_log_domain() {
        let prev = 1.0f32.ln();
        let next = adapt(prev, 4.0, 0.2, 0.02);
        assert!(close(next, prev + 0.02 * (4f32.ln() - prev), 1e-6));
        assert_eq!(adapt(prev, 4.0, 1e-6, 0.02), prev);
    }

    #[test]
    fn switch_parses() {
        assert_eq!(ToneMath::parse("exact"), ToneMath::Exact);
        assert_eq!(ToneMath::parse(" EXACT "), ToneMath::Exact);
        assert_eq!(ToneMath::parse("fitted"), ToneMath::Fitted);
        assert_eq!(ToneMath::parse(""), ToneMath::Exact);
    }

    #[test]
    fn the_two_maths_differ_but_both_scale_by_the_host_exposure() {
        // Fitted and exact are different curves; the host's exposure scales
        // both the same way (twice the exposure, twice the first-row gain).
        let ls = with(&[("tonemap", 2.0)]);
        let a = filmic_buffer_for(ToneMath::Exact, &ls, 1.0, false);
        let b = filmic_buffer_for(ToneMath::Exact, &ls, 2.0, false);
        assert!(close(b[2][0], 2.0 * a[2][0], 1e-6));
        let f1 = filmic_buffer_for(ToneMath::Fitted, &ls, 1.0, false);
        let f2 = filmic_buffer_for(ToneMath::Fitted, &ls, 2.0, false);
        assert!(close(f2[1][0], 2.0 * f1[1][0], 1e-6));
    }

    #[test]
    fn vision_line_names_the_map() {
        let l = vision_line(
            "mp_x",
            &Settings::default(),
            &Settings::default(),
            Some(0.36),
        );
        assert!(l.starts_with("h1 vision: mp_x"), "{l}");
        assert!(l.contains("x0.500"), "{l}");
    }
}
