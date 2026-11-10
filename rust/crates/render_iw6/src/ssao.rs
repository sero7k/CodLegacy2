//! Screen-space ambient occlusion with Ghosts' own passes: `ssao_calc_fast`
//! (or `ssao_calc_slow`) over `FLOATZ` and the engine's `RANDOM_ROTATIONS`
//! picture into a one-channel target, `ssao_vblur_lq` (a bilateral blur
//! along y) into a second, then the post-opaque pass's `postopaquefx_ssao*`
//! form (the fog and colorization pass we already run, plus the AO and the
//! blur along x, in one shader).
//!
//! READ from the engine (`docs/exe_notes/IW6_SHADERS.md` 13, `IW6_POST.md`):
//! the six constants and how the dvars feed them, the blur steps, the
//! rotation picture, the `r_ssao` mode (1 uses the fast pass), the target
//! size (the render size; one channel, 8 bits). Strength and power come
//! from the vision (`r_ssaoStrength`, `r_ssaoPower`) and no strength means
//! no pass; `r_ssaoUseTweaks` (the dvars instead) is not driven.
//!
//! INFERRED: the half-size target of the HIGH preset (the engine's is the
//! render size, kept on ULTRA), the blur pass count (the engine runs the
//! second blur inside the composite shader: its constants carry the x step).

use crate::code::CodeValues;
use crate::look::Vision;
use render_h1::gpu::{Gpu, Tex};

/// `BO2MP_IW6_SSAO=0`: no ambient occlusion whatever the preset says.
pub fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_SSAO");
    S.get()
}

/// `BO2MP_IW6_SSAO_MODE=2`: `r_ssao` mode 2 (`ssao_calc_slow`); default 1.
pub fn mode() -> u8 {
    static M: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *M.get_or_init(|| match std::env::var("BO2MP_IW6_SSAO_MODE").ok().and_then(|v| v.trim().parse::<u8>().ok()) {
        Some(2) => 2,
        _ => 1,
    })
}

/// The calc technique of the mode.
pub fn calc_technique() -> &'static str {
    if mode() == 2 { "ssao_calc_slow" } else { "ssao_calc_fast" }
}

/// The target's size for a frame of `w` x `h`: the render size (READ), or
/// half of it (`half`: the HIGH preset, INFERRED).
pub fn target_size(w: u32, h: u32, half: bool) -> (u32, u32) {
    if half { (w.div_ceil(2).max(1), h.div_ceil(2).max(1)) } else { (w.max(1), h.max(1)) }
}

/// The `r_ssao*` values (dvar defaults, READ from the registration) with
/// the vision's strength and power.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub strength: f32,
    pub power: f32,
    pub diminish: f32,
    /// `r_ssaoWidth` over 720.
    pub width: f32,
    pub depth_scale: f32,
    pub depth_scale_view_model: f32,
    pub blur_sharpness: f32,
    pub blur_step: f32,
    pub gap_falloff: f32,
    pub gradient_falloff: f32,
    pub fade_depth: f32,
    pub reject_depth: f32,
    pub min_pixel_width: f32,
}

/// The first and second blur's step table (READ, mode 1 and 2: both 2).
const STEP_TABLE: f32 = 2.0;

impl Params {
    pub fn from_vision(v: &Vision) -> Self {
        Self {
            strength: v.f("r_ssaoStrength", 0.0),
            power: v.f("r_ssaoPower", 1.0),
            diminish: 1.0,
            width: 36.0 / 720.0,
            depth_scale: 0.1,
            depth_scale_view_model: 0.0003,
            blur_sharpness: 50.0,
            blur_step: 1.0,
            gap_falloff: 0.25,
            gradient_falloff: 0.05,
            fade_depth: 2500.0,
            reject_depth: 2500.0,
            min_pixel_width: 1.0,
        }
    }

    /// The engine draws nothing for a strength of zero (mode 0).
    pub fn active(&self) -> bool {
        self.strength > 0.0
    }

    /// The blur step in texels: `max(1, floor(r_ssaoBlurStep * 2 + 0.5))`.
    pub fn step(&self) -> f32 {
        (self.blur_step * STEP_TABLE + 0.5).floor().max(1.0)
    }

    /// The calc pass's and the first blur's constants for a target of
    /// `w` x `h` (`RENDER_TARGET_SIZE` too: the pass's own target).
    pub fn calc_consts(&self, w: u32, h: u32, code: &mut CodeValues) {
        let (wf, hf) = (w as f32, h as f32);
        let k = hf * self.width / wf;
        let reject = self.reject_depth.min(hf * self.width * self.fade_depth / self.min_pixel_width);
        let sd = self.strength * self.diminish;
        code.set_vec("RENDER_TARGET_SIZE", [wf, hf, 1.0 / wf, 1.0 / hf]);
        // (power, sharpness, step along x, step along y): the first blur runs along y.
        code.set_vec("SSAO_POWER_BB_SHARPNESS_STEP", [self.power, self.blur_sharpness, 0.0, self.step() / hf]);
        code.set_vec("SSAO_COLOR_COEFF", [sd * 0.299, sd * 0.587, sd * 0.114, self.strength]);
        code.set_vec(
            "SSAO_FALLOFF_DEPTH_SCALE",
            [-1.0 / self.gap_falloff, self.gradient_falloff * self.gradient_falloff, self.depth_scale, self.depth_scale_view_model * self.depth_scale],
        );
        code.set_vec("SSAO_UV_TO_ROT_SCALE_AND_FADE", [wf * 0.125, hf * 0.125, self.fade_depth, 0.0]);
        code.set_vec("SSAO_SAMPLE_MAT_SCALE", [k, k, -self.width, self.width]);
        code.set_vec("SSAO_DEPTH_OFFSET_REJECT", [1.0 / wf, 1.0 / hf, 0.0, 1.0 / reject]);
    }

    /// The composite's: the second blur along x (`step / w`).
    pub fn composite_consts(&self, w: u32, code: &mut CodeValues) {
        let sd = self.strength * self.diminish;
        code.set_vec("SSAO_POWER_BB_SHARPNESS_STEP", [self.power, self.blur_sharpness, self.step() / w as f32, 0.0]);
        code.set_vec("SSAO_COLOR_COEFF", [sd * 0.299, sd * 0.587, sd * 0.114, self.strength]);
    }
}

/// The post-opaque technique with the AO: `postopaquefx_dfog_color_altd`
/// -> `postopaquefx_ssaolq_dfog_color_altd`.
pub fn composite_technique(fog: &str) -> String {
    match fog.strip_prefix("postopaquefx_") {
        Some(rest) => format!("postopaquefx_ssaolq_{rest}"),
        None => fog.to_string(),
    }
}

/// `RANDOM_ROTATIONS` (READ, 8 x 8, R16G16_FLOAT, 64 unit vectors).
pub const ROTATIONS: [[f32; 2]; 64] = [
    [0.3188, 0.9478], [0.8145, -0.5801], [0.9653, 0.2615], [0.6572, 0.7534], [-0.9653, -0.2615], [-0.9917, 0.1278], [0.9746, -0.2244], [0.9863, 0.1655],
    [-0.8672, 0.4976], [0.7280, 0.6855], [-0.9478, 0.3188], [0.7534, -0.6572], [-0.3547, 0.9351], [0.4102, 0.9121], [-0.5801, -0.8145], [-0.9863, -0.1655],
    [-0.6855, 0.7280], [-0.9976, -0.0681], [0.7915, 0.6108], [-0.2615, 0.9653], [0.3547, -0.9351], [0.9478, -0.3188], [-0.9121, 0.4102], [-0.4976, -0.8672],
    [0.5801, 0.8145], [-0.5303, 0.8477], [0.0300, 0.9995], [0.1278, 0.9917], [0.4976, 0.8672], [-0.0300, -0.9995], [0.8672, -0.4976], [-0.7915, -0.6108],
    [0.8955, 0.4448], [0.0681, -0.9976], [-0.4102, -0.9121], [0.2244, 0.9746], [-0.9746, 0.2244], [-0.2244, -0.9746], [0.9976, 0.0681], [-0.6572, -0.7534],
    [0.5303, -0.8477], [-0.8477, -0.5303], [-0.1278, -0.9917], [0.9351, 0.3547], [-0.0681, 0.9976], [0.9121, -0.4102], [0.9917, -0.1278], [-0.9351, -0.3547],
    [-0.4448, 0.8955], [-0.8145, 0.5801], [0.4448, -0.8955], [0.6855, -0.7280], [0.6108, -0.7915], [0.1655, -0.9863], [-0.6108, 0.7915], [0.9995, -0.0300],
    [-0.1655, 0.9863], [-0.9995, 0.0300], [0.8477, 0.5303], [-0.3188, -0.9478], [0.2615, -0.9653], [-0.7534, 0.6572], [-0.8955, -0.4448], [-0.7280, -0.6855],
];

/// A float as IEEE half bits (round to nearest even; the rotations are
/// well inside the normal range).
pub fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x007f_ffff;
    if x == 0.0 {
        return sign;
    }
    if exp <= 0 {
        return sign; // (flushed: no value of the table is this small)
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    let half = (exp as u32) << 10 | (man >> 13);
    let round = (man & 0x1fff > 0x1000) || (man & 0x1fff == 0x1000 && half & 1 == 1);
    sign | (half + u32::from(round)) as u16
}

/// The picture as the engine makes it, sampled wrapped and unfiltered.
pub fn rotations(gpu: &Gpu) -> Tex {
    let mut bytes = Vec::with_capacity(64 * 4);
    for r in ROTATIONS {
        bytes.extend_from_slice(&f16_bits(r[0]).to_le_bytes());
        bytes.extend_from_slice(&f16_bits(r[1]).to_le_bytes());
    }
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("iw6_random_rotations"),
        size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(32), rows_per_image: Some(8) },
        wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
    );
    let view = texture.create_view(&Default::default());
    Tex { texture, view, dim: wgpu::TextureViewDimension::D2 }
}

/// The two one-channel targets (calc, blur) and the rotations.
pub struct Targets {
    pub size: (u32, u32),
    pub ao: [wgpu::TextureView; 2],
}

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

impl Targets {
    pub fn new(gpu: &Gpu, w: u32, h: u32) -> Self {
        let ao = [0, 1].map(|_| render_h1::post::target(gpu, "iw6_ssao", w, h, FORMAT).create_view(&Default::default()));
        Self { size: (w, h), ao }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vision() -> Vision {
        Vision::parse("r_ssaoStrength 0.35\nr_ssaoPower 15.99\n")
    }

    #[test]
    fn constants_follow_the_engine_arithmetic() {
        let p = Params::from_vision(&vision());
        assert!(p.active());
        assert_eq!(p.step(), 2.0);
        let mut c = CodeValues::default();
        p.calc_consts(1280, 720, &mut c);
        let row = |n: &str| c.get(n).unwrap()[0];
        assert_eq!(row("SSAO_POWER_BB_SHARPNESS_STEP"), [15.99, 50.0, 0.0, 2.0 / 720.0]);
        let col = row("SSAO_COLOR_COEFF");
        assert!((col[0] - 0.35 * 0.299).abs() < 1e-6 && col[3] == 0.35);
        assert_eq!(row("SSAO_FALLOFF_DEPTH_SCALE")[0], -4.0);
        assert!((row("SSAO_FALLOFF_DEPTH_SCALE")[1] - 0.0025).abs() < 1e-7);
        assert!((row("SSAO_FALLOFF_DEPTH_SCALE")[3] - 0.00003).abs() < 1e-8);
        assert_eq!(row("SSAO_UV_TO_ROT_SCALE_AND_FADE"), [160.0, 90.0, 2500.0, 0.0]);
        // W = 36 / 720 = 0.05; k = W h / w.
        let m = row("SSAO_SAMPLE_MAT_SCALE");
        assert!((m[0] - 0.05 * 720.0 / 1280.0).abs() < 1e-6 && m[0] == m[1]);
        assert_eq!((m[2], m[3]), (-0.05, 0.05));
        // reject = min(2500, 720 * 0.05 * 2500 / 1) = 2500.
        assert_eq!(row("SSAO_DEPTH_OFFSET_REJECT"), [1.0 / 1280.0, 1.0 / 720.0, 0.0, 1.0 / 2500.0]);
        p.composite_consts(1280, &mut c);
        assert_eq!(c.get("SSAO_POWER_BB_SHARPNESS_STEP").unwrap()[0], [15.99, 50.0, 2.0 / 1280.0, 0.0]);
    }

    #[test]
    fn no_strength_no_ssao() {
        assert!(!Params::from_vision(&Vision::parse("")).active());
        assert!(!Params::from_vision(&Vision::parse("r_ssaoStrength 0")).active());
    }

    #[test]
    fn names_and_sizes() {
        assert_eq!(composite_technique("postopaquefx_dfog_color_altd"), "postopaquefx_ssaolq_dfog_color_altd");
        assert_eq!(composite_technique("postopaquefx_fog"), "postopaquefx_ssaolq_fog");
        assert_eq!(target_size(1281, 721, true), (641, 361));
        assert_eq!(target_size(1281, 721, false), (1281, 721));
    }

    #[test]
    fn half_floats_of_the_table() {
        // 0.5 = 0x3800, 1.0 = 0x3c00, -1.0 = 0xbc00.
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(-1.0), 0xbc00);
        assert_eq!(f16_bits(0.0), 0);
        // Every table entry is a unit vector to the printed precision.
        for r in ROTATIONS {
            assert!((r[0] * r[0] + r[1] * r[1] - 1.0).abs() < 2e-3, "{r:?}");
        }
        // And survives the round trip to half precision.
        for r in ROTATIONS {
            for v in r {
                let b = f16_bits(v);
                let back = {
                    let s = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
                    let e = ((b >> 10) & 0x1f) as i32;
                    let m = (b & 0x3ff) as f32;
                    if e == 0 { s * m * 2f32.powi(-24) } else { s * (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
                };
                assert!((back - v).abs() < 6e-4, "{v} {back}");
            }
        }
    }
}
