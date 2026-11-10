//! Depth of field with Ghosts' own passes (the engine's `r_dof_hq` 0 chain):
//! `dof_downsample` (the scene to quarter size, the circle of confusion in
//! alpha, from FLOATZ), a blur of that, `dof_near_coc` (spreads the near
//! circle), then `postfx_dof` (the scene, the sharp downsample and the
//! blurred one weighted by the circle at each pixel) into the frame.
//!
//! READ from the engine (`docs/exe_notes/IW6_POST.md` 3): the eight
//! distances (view model start / end, near start / end, far start / end,
//! near blur, far blur: `Dof`), when the effect runs (`Dof::active`), how
//! they become `DOF_EQUATION_SCENE`, `DOF_EQUATION_VIEWMODEL_AND_FAR_BLUR`,
//! `DOF_LERP_SCALE`, `DOF_LERP_BIAS` and `DOF_ROW_DELTA` (with `r_dof_bias`,
//! default 0.5, as a power), the quarter-size targets and the blur radius
//! `near blur / 4 * height / 480` target pixels.
//!
//! INFERRED: the blur is the material bloom's seven-tap blur (the engine's is
//! a generated gaussian kernel), which texture `postfx_dof` takes as
//! `POST_EFFECT_0` (the blur) and `POST_EFFECT_1` (the spread near circle),
//! that the engine's height in the equations is the render height, and the
//! guards on the bias powers.

use crate::code::CodeValues;
use render_h1::gpu::Gpu;

/// `BO2MP_IW6_DOF=0`: no depth of field whatever the preset says.
pub fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_DOF");
    S.get()
}

/// `r_dof_bias`' default.
pub const BIAS: f32 = 0.5;

/// The eight distances a script (or the weapon's ADS) sets, in the
/// engine's order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dof {
    pub view_model_start: f32,
    pub view_model_end: f32,
    pub near_start: f32,
    pub near_end: f32,
    pub far_start: f32,
    pub far_end: f32,
    pub near_blur: f32,
    pub far_blur: f32,
}

impl Dof {
    pub fn from_array(a: [f32; 8]) -> Self {
        Self {
            view_model_start: a[0],
            view_model_end: a[1],
            near_start: a[2],
            near_end: a[3],
            far_start: a[4],
            far_end: a[5],
            near_blur: a[6],
            far_blur: a[7],
        }
    }

    /// READ: the view model range, the near range or the far range (with a
    /// far blur) is more than a unit wide.
    pub fn active(&self) -> bool {
        self.view_model_start + 1.0 < self.view_model_end
            || self.near_start + 1.0 < self.near_end
            || (self.far_start + 1.0 < self.far_end && self.far_blur > 0.0)
    }

    /// The blur's radius in quarter-size target pixels.
    pub fn blur_radius(&self, height: f32) -> f32 {
        self.near_blur.max(0.0) * 0.25 * height / 480.0
    }

    /// The pass constants (`near`: the camera's near plane, `height`: the
    /// render height).
    pub fn consts(&self, near: f32, height: f32, bias: f32, code: &mut CodeValues) {
        let nb = self.near_blur.max(1e-3);
        // Scene: near and far ramps as `scale * distance + bias` weights.
        let (mut ns, mut ne) = (self.near_start, self.near_end);
        if ne <= near.max(ns) {
            ns = 0.0;
            ne = near * 0.5;
        }
        let near_scale = near * 0.984375 / (ns - ne);
        let near_bias = ne / (ne - ns);
        let (fs, fe) = (self.far_start, self.far_end);
        let (far_scale, far_bias) = if near.max(fs) < fe { (near * 0.984375 / (fe - fs), fs / (fs - fe)) } else { (0.0, 0.0) };
        code.set_vec("DOF_EQUATION_SCENE", [near_scale, far_scale, near_bias, far_bias]);
        // View model and the far blur's weight.
        let (mut vs, mut ve) = (self.view_model_start, self.view_model_end);
        if ve <= vs.max(0.1) {
            ve = 0.05;
            vs = 0.0;
        }
        let ratio = (self.far_blur / nb).max(0.0).powf(bias);
        code.set_vec("DOF_EQUATION_VIEWMODEL_AND_FAR_BLUR", [0.0015625 / (vs - ve), 0.984375, ve / (ve - vs), ratio]);
        // The blur weights' ramp: 672 / height and 1728 / height of the
        // near blur, as a power.
        let p5 = ((672.0 / height) / nb).powf(bias).min(0.998);
        let p6 = ((1728.0 / height) / nb).powf(bias).clamp(p5 + 1e-3, 0.999);
        let r = 1.0 - p6;
        code.set_vec("DOF_LERP_SCALE", [-1.0 / p5.max(1e-4), -1.0 / (p6 - p5), -1.0 / r, 1.0 / r]);
        code.set_vec("DOF_LERP_BIAS", [1.0, p6 / (p6 - p5), 1.0 / r, -p6 / r]);
        code.set_vec("DOF_ROW_DELTA", [0.0, 1.0 / height, 0.0, 0.0]);
    }
}

/// The engine's generated gaussian kernel (READ from its blur-kernel
/// builder, `docs/exe_notes/IW6_POST.md` 3): taps are folded in pairs so a
/// bilinear fetch between two texels reads both. Pair `i` covers the
/// distances `2i + s` and `2i + 1 + s` (texels), `s` = 0.5 when
/// `round(size / factor)` is even, else 0; its weight is
/// `g(a) + g(b)` with `g(x) = exp(-x^2 / (2 sigma^2))` (the very first
/// distance, 0, counts half), its offset the weighted mean distance over
/// `size` (a UV step; `(a + b) / 2 / size` when both weights vanish).
/// Returns the sum of the raw weights; `offsets` and `weights` take the
/// first `min(len)` pairs. No allocation.
pub fn gaussian_pairs(sigma: f32, size: i32, factor: i32, offsets: &mut [f32], weights: &mut [f32]) -> f32 {
    let size_f = size as f32;
    let shift = if ((size_f / factor.max(1) as f32 + 0.5).floor() as i32) & 1 == 0 { 0.5 } else { 0.0 };
    let k = -0.5 / (sigma * sigma);
    let mut sum = 0.0;
    for (i, (o, w)) in offsets.iter_mut().zip(weights.iter_mut()).enumerate() {
        let a = (i * 2) as f32 + shift;
        let b = (i * 2 + 1) as f32 + shift;
        let mut ga = (a * k * a).exp();
        let gb = (b * k * b).exp();
        if i == 0 && shift == 0.0 {
            ga *= 0.5;
        }
        let pair = ga + gb;
        *w = pair;
        *o = if pair == 0.0 { (a + b) * 0.5 / size_f } else { (gb * b + ga * a) / (pair * size_f) };
        sum += pair;
    }
    sum
}

/// The kernel's normalisation (READ): every weight times `0.5 / sum` (the
/// pairs are used on both sides). Returns how many pairs to use: one past
/// the lowest pair whose normalised weight is under 0.01 (all of them when
/// none is), or 1 with a single 0.5 weight when the sum is negligible.
pub fn normalise(weights: &mut [f32], sum: f32) -> usize {
    if sum <= 0.001 {
        if let Some(w) = weights.first_mut() {
            *w = 0.5;
        }
        return 1;
    }
    let k = 0.5 / sum;
    let mut used = weights.len();
    for (i, w) in weights.iter_mut().enumerate().rev() {
        *w *= k;
        if *w < 0.01 {
            used = i + 1;
        }
    }
    used
}

/// The quarter-size targets: the downsample, the blur's scratch, the blur
/// and the near-spread copy.
pub struct Targets {
    pub size: (u32, u32),
    pub down: wgpu::TextureView,
    pub scratch: wgpu::TextureView,
    pub blur: wgpu::TextureView,
    pub near: wgpu::TextureView,
}

impl Targets {
    pub fn new(gpu: &Gpu, w: u32, h: u32, format: wgpu::TextureFormat) -> Self {
        let size = (w.div_ceil(4).max(1), h.div_ceil(4).max(1));
        let mk = |n: &str| render_h1::post::target(gpu, n, size.0, size.1, format).create_view(&Default::default());
        Self { size, down: mk("iw6_dof_down"), scratch: mk("iw6_dof_scratch"), blur: mk("iw6_dof_blur"), near: mk("iw6_dof_near") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dof() -> Dof {
        Dof::from_array([0.0, 0.0, 5.0, 40.0, 200.0, 800.0, 4.0, 4.0])
    }

    fn get(code: &CodeValues, name: &str) -> [f32; 4] {
        code.get(name).map(|v| v[0]).unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn active_follows_the_ranges() {
        assert!(dof().active());
        assert!(!Dof::default().active());
        // A far range with no far blur does nothing.
        assert!(!Dof::from_array([0.0, 0.0, 0.0, 0.0, 100.0, 500.0, 4.0, 0.0]).active());
        assert!(Dof::from_array([0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0]).active());
    }

    #[test]
    fn near_ramp_is_one_at_start_and_zero_at_the_end() {
        // The weight is `scale / floatz + bias`, FLOATZ = near * 63/64 / distance.
        let mut c = CodeValues::default();
        let near = 4.0;
        dof().consts(near, 720.0, BIAS, &mut c);
        let e = get(&c, "DOF_EQUATION_SCENE");
        let z = |d: f32| near * 0.984375 / d;
        let near_w = |d: f32| e[0] / z(d) + e[2];
        assert!((near_w(5.0) - 1.0).abs() < 1e-4, "{}", near_w(5.0));
        assert!(near_w(40.0).abs() < 1e-4, "{}", near_w(40.0));
        let far_w = |d: f32| e[1] / z(d) + e[3];
        assert!(far_w(200.0).abs() < 1e-4 && (far_w(800.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn lerp_constants_are_finite_for_any_blur() {
        for nb in [0.0, 0.5, 1.0, 4.0, 20.0] {
            let mut c = CodeValues::default();
            Dof::from_array([0.0, 0.0, 5.0, 40.0, 200.0, 800.0, nb, 4.0]).consts(4.0, 720.0, BIAS, &mut c);
            for n in ["DOF_LERP_SCALE", "DOF_LERP_BIAS", "DOF_EQUATION_VIEWMODEL_AND_FAR_BLUR", "DOF_EQUATION_SCENE"] {
                assert!(get(&c, n).iter().all(|v| v.is_finite()), "{n} at blur {nb}");
            }
        }
    }

    #[test]
    fn blur_radius_scales_with_height() {
        assert!((dof().blur_radius(720.0) - 1.5).abs() < 1e-6);
        assert!((dof().blur_radius(1440.0) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn gaussian_pairs_are_a_normalised_folded_gaussian() {
        let (mut o, mut w) = ([0.0f32; 16], [0.0f32; 16]);
        let sigma = 3.0;
        let sum = gaussian_pairs(sigma, 360, 4, &mut o, &mut w);
        // round(360 / 4) = 90 is even: the pairs sit at half texels, the
        // first distance (0.5) is not halved.
        let g = |x: f32| (-0.5 * x * x / (sigma * sigma)).exp();
        assert!((w[0] - (g(0.5) + g(1.5))).abs() < 1e-6);
        assert!((o[0] - (g(0.5) * 0.5 + g(1.5) * 1.5) / ((g(0.5) + g(1.5)) * 360.0)).abs() < 1e-7);
        let used = normalise(&mut w, sum);
        // Both sides use the pairs: the weights add up to one half.
        assert!((w.iter().sum::<f32>() - 0.5).abs() < 1e-5);
        assert!(used >= 1 && used <= 16);
        // Offsets grow with the pair.
        assert!(o.windows(2).all(|p| p[1] > p[0]));
        // An odd round(size / factor): the first pair is distances 0 and 1,
        // and distance 0 counts half.
        let sum = gaussian_pairs(sigma, 404, 4, &mut o, &mut w);
        assert!((w[0] - (0.5 + g(1.0))).abs() < 1e-6, "{}", w[0]);
        assert!(sum > 0.0);
    }

    #[test]
    fn a_vanishing_kernel_is_one_half_weight() {
        let mut w = [0.0f32; 4];
        assert_eq!(normalise(&mut w, 0.0), 1);
        assert_eq!(w[0], 0.5);
    }
}
