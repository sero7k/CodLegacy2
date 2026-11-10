//! MWR's bloom ("veil"): the pass plan its renderer runs, read from the
//! game's executable (`docs/H1_NATIVE.md`, "Bloom chain"). This builds the
//! list of full-screen passes (which material, which source and target
//! level, which `VEIL_PARAMS`); it draws nothing itself.
//!
//! READ: a first pass from the scene into level 0 (material by the
//! downsample kind), then one downsample per further level, either a plain
//! linear downsample or a separable blur (a vertical then a horizontal
//! gaussian pass through a temporary of the source's size); then the way
//! back up, level by level, blending each level with the one below by
//! weights, again plain or blurred. The mode decides which levels blur.
//!
//! Level numbering here: `Down(0)` is the first pass's target (the exe's
//! `$veil_mip1`), `Down(i)` the mip `i + 1` (8 in all), `Up(l)` the upsample
//! target blended toward `Down(l)` (`Up(1)` is what the colour pass reads),
//! `Temp` the blur temporaries.
//!
//! INFERRED (marked in the code): the engine's level count (8, the exe's
//! cap), the light-set level floor (0), `FUN_14077ce34` as a square root,
//! and the third downsample kind's name (Karis, from its material).

/// How the blur is spread over the pyramid (`Upscale` is the default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Plain downsample, blur while upsampling.
    Upscale,
    /// Blur while downsampling, plain upsample.
    Downscale,
    /// Blur every other level on each leg.
    HybridFast,
    /// Blur on both legs, step scaled.
    HybridReference,
}

impl Mode {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Downscale,
            2 => Self::HybridFast,
            3 => Self::HybridReference,
            _ => Self::Upscale,
        }
    }
}

/// The first pass's material kind (`veil_downsample_*`); default log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Downsample {
    Linear,
    Log,
    Karis,
}

impl Downsample {
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => Self::Linear,
            2 => Self::Karis,
            _ => Self::Log,
        }
    }

    pub fn material(self) -> &'static str {
        match self {
            Self::Linear => "veil_downsample_linear",
            Self::Log => "veil_downsample_log",
            Self::Karis => "veil_downsample_karis",
        }
    }
}

/// The gaussian's size (`low`, `med`, `high`); default med.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    Med,
    High,
}

impl Quality {
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => Self::Low,
            2 => Self::High,
            _ => Self::Med,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Med => "med",
            Self::High => "high",
        }
    }
}

/// The veil's dvars at their defaults (the exe registers them by hash, so
/// the names are not known; values and ranges read).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub downsample: Downsample,
    pub quality: Quality,
    /// The blur radius (default 8, range 0..100).
    pub radius: f32,
    /// Blend weights of the upsample, per level (default 0.25, 0.75, 1.5,
    /// 2, 2.5, 3), normalised by their sum when it is positive.
    pub weights: [f32; 6],
    /// The blur step scale per level (default 1, range 0..8).
    pub steps: [f32; 6],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Upscale,
            downsample: Downsample::Log,
            quality: Quality::Med,
            radius: 8.0,
            weights: [0.25, 0.75, 1.5, 2.0, 2.5, 3.0],
            steps: [1.0; 6],
        }
    }
}

impl Settings {
    /// The weights as the exe's table setup leaves them.
    pub fn normalised_weights(&self) -> [f32; 6] {
        let sum: f32 = self.weights.iter().sum();
        if sum > 0.0 { self.weights.map(|w| w / sum) } else { self.weights }
    }
}

/// A render target of the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// The scene.
    Scene,
    /// A downsample target (0 = the first pass's).
    Down(u32),
    /// An upsample target.
    Up(u32),
    /// The blur temporary of a down or up level.
    Temp(Which),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Which {
    Down(u32),
    Up(u32),
}

/// One full-screen pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub material: String,
    pub src: Level,
    /// The second source of an adding pass.
    pub src2: Option<Level>,
    pub dst: Level,
    /// `VEIL_PARAMS`: the step (x, y) as a fraction of the *target's*
    /// size over 3, and two weights.
    pub params: [f32; 4],
    /// Plain upsample passes also write (w, w, w, w2) to a second constant
    /// (`MATERIAL_COLOR`: the zone's `veil_bilinear` reads it).
    pub plain_weights: Option<[f32; 4]>,
}

/// The blur step the exe derives from the radius and the view:
/// `y = height * radius / 480`, `x = y / aspect`, `aspect = height *
/// pixel aspect / width`.
pub fn blur_step(radius: f32, width: f32, height: f32, pixel_aspect: f32) -> (f32, f32) {
    let y = height * radius / 480.0;
    let aspect = height * pixel_aspect / width;
    (y / aspect, y)
}

fn size_at(w: u32, h: u32, level: u32) -> (f32, f32) {
    // Level k is the scene halved k + 1 times, never below one texel
    // (INFERRED: the exe keeps the sizes in a table filled at start).
    ((w >> (level + 1)).max(1) as f32, (h >> (level + 1)).max(1) as f32)
}

/// The target a pass writes, as (width, height) in texels. `Up(l)` is as
/// large as `Down(l)` (it is blended toward it); a down temporary is as
/// large as the level it reads (`Down(i + 1)`).
pub fn level_size(w: u32, h: u32, l: Level) -> (f32, f32) {
    match l {
        Level::Scene => (w as f32, h as f32),
        Level::Down(i) | Level::Up(i) | Level::Temp(Which::Up(i)) => size_at(w, h, i),
        Level::Temp(Which::Down(i)) => size_at(w, h, i + 1),
    }
}

/// A separable blur through a temporary: a pass along `first` into the
/// temporary, then along the other axis into `dst`.
#[allow(clippy::too_many_arguments)]
fn blur(
    out: &mut Vec<Pass>,
    size: (u32, u32),
    q: Quality,
    src: Level,
    src2: Option<Level>,
    dst: Level,
    tmp: Level,
    step: (f32, f32),
    weights: (f32, f32),
    x_first: bool,
) {
    let (tw, th) = level_size(size.0, size.1, tmp);
    let (dw, dh) = level_size(size.0, size.1, dst);
    // First pass: along one axis only, sized by the temporary.
    let (sx1, sy1) = if x_first { (step.0, 0.0) } else { (0.0, step.1) };
    out.push(Pass {
        material: format!("veil_gaussian_{}", q.name()),
        src,
        src2: None,
        dst: tmp,
        params: [sx1 / tw / 3.0, sy1 / th / 3.0, 1.0, 1.0],
        plain_weights: None,
    });
    // Second pass: the other axis, sized by the destination, carrying the
    // weights (and the second source on the way up).
    let (sx2, sy2) = if x_first { (0.0, step.1) } else { (step.0, 0.0) };
    out.push(Pass {
        material: if src2.is_some() {
            format!("veil_gaussian_add_{}", q.name())
        } else {
            format!("veil_gaussian_{}", q.name())
        },
        src: tmp,
        src2,
        dst,
        params: [sx2 / dw / 3.0, sy2 / dh / 3.0, weights.0, weights.1],
        plain_weights: None,
    });
}

/// The whole chain for a `width` x `height` scene.
///
/// `levels`: the engine's level count (INFERRED 8, the cap); the chain
/// runs `min(8, levels) - 2` levels, at least `floor - 2` (the light set's
/// own count, INFERRED 0).
pub fn plan(s: &Settings, width: u32, height: u32, pixel_aspect: f32, levels: u32) -> Vec<Pass> {
    let n = levels.min(8) as i32;
    let count = (n - 2).max(0) as u32;
    let (rx, ry) = blur_step(s.radius, width as f32, height as f32, pixel_aspect);
    let w = s.normalised_weights();
    let mut out = Vec::new();
    // First pass: the scene into level 0.
    out.push(Pass {
        material: s.downsample.material().into(),
        src: Level::Scene,
        src2: None,
        dst: Level::Down(0),
        params: [0.0; 4],
        plain_weights: None,
    });
    // The first plain downsample, before the exe's loop.
    out.push(Pass {
        material: "veil_downsample_linear".into(),
        src: Level::Down(0),
        src2: None,
        dst: Level::Down(1),
        params: [0.0; 4],
        plain_weights: None,
    });
    // Downsample leg. The exe scales the step by 1 / 2^3 here.
    let down_k = 1.0 / 2f32.powf(3.0);
    for i in 0..count {
        let sc = s.steps[(i as usize).min(5)];
        let (mut sx, mut sy) = (rx * down_k * sc, ry * down_k * sc);
        let blur_it = match s.mode {
            Mode::Upscale => false,
            Mode::Downscale => true,
            Mode::HybridFast => i != 0 && i % 2 == 0,
            Mode::HybridReference => true,
        };
        // Scaled by 1 / sqrt(2) (INFERRED: the square root) in the modes
        // that blur every level.
        if matches!(s.mode, Mode::Downscale | Mode::HybridReference) {
            let k = 1.0 / 2f32.sqrt();
            sx *= k;
            sy *= k;
        }
        if blur_it {
            blur(
                &mut out,
                (width, height),
                s.quality,
                Level::Down(i + 1),
                None,
                Level::Down(i + 2),
                Level::Temp(Which::Down(i + 1)),
                (sx, sy),
                (1.0, 1.0),
                false,
            );
        } else {
            out.push(Pass {
                material: "veil_downsample_linear".into(),
                src: Level::Down(i + 1),
                src2: None,
                dst: Level::Down(i + 2),
                params: [0.0; 4],
                plain_weights: None,
            });
        }
    }
    // Upsample leg, from the smallest level back.
    let up_k = 1.0 / 2f32.powf(2.0);
    for u in 0..count {
        let l = count - u; // the exe's level, descending
        let last = u == count - 1;
        let cur_w = if u == 0 { w[(l as usize - 1).min(5)] } else { 1.0 };
        let other_w = if last { 1.0 } else { w[(l as usize).saturating_sub(2).min(5)] };
        let src = if u == 0 { Level::Down(l + 1) } else { Level::Up(l + 1) };
        let src2 = if last { None } else { Some(Level::Down(l)) };
        let dst = Level::Up(l);
        let sc = s.steps[(l as usize - 1).min(5)];
        let (mut sx, mut sy) = (rx * up_k * sc, ry * up_k * sc);
        let blur_it = match s.mode {
            Mode::Upscale => true,
            Mode::Downscale => false,
            Mode::HybridFast => last || u % 2 == 0,
            Mode::HybridReference => true,
        };
        if matches!(s.mode, Mode::HybridFast | Mode::HybridReference) && !(matches!(s.mode, Mode::HybridFast) && !last && u % 2 == 0) {
            // The exe scales the step by 1 / sqrt(2) in the branches that
            // are not the parity one.
            let k = 1.0 / 2f32.sqrt();
            sx *= k;
            sy *= k;
        }
        if blur_it {
            blur(
                &mut out,
                (width, height),
                s.quality,
                src,
                src2,
                dst,
                Level::Temp(Which::Up(l)),
                (sx, sy),
                (cur_w, other_w),
                true,
            );
        } else {
            out.push(Pass {
                material: if src2.is_some() { "veil_bilinear_add" } else { "veil_bilinear" }.into(),
                src,
                src2,
                dst,
                params: [0.0; 4],
                plain_weights: Some([cur_w, cur_w, cur_w, other_w]),
            });
        }
    }
    out
}


/// Whether the bloom chain and veil colour pass run. `BO2MP_H1_VEIL=1`
/// forces it on, `0` forces it off; otherwise it follows the map's light
/// set (`veil` = 1), as the game does. Checked on a software-GPU render of
/// mp_rust (veilstrength 0.139, background 0.9): a soft glow, no wash.
pub fn enabled(lightset_veil: bool) -> bool {
    match std::env::var("BO2MP_H1_VEIL").ok().as_deref().map(str::trim) {
        Some("1") => true,
        Some("0") => false,
        _ => lightset_veil,
    }
}

#[cfg(test)]
mod enabled_tests {
    use super::enabled;
    #[test]
    fn follows_lightset_without_env() {
        if std::env::var("BO2MP_H1_VEIL").is_err() {
            assert!(enabled(true));
            assert!(!enabled(false));
        }
    }
}

/// The chain's render targets for one size, and the code to run a plan.
pub struct Chain {
    width: u32,
    height: u32,
    views: std::collections::HashMap<Level, wgpu::TextureView>,
    _textures: Vec<wgpu::Texture>,
    /// The last pass's target: what the colour pass reads (`Up(1)` for a
    /// whole plan; a shorter test plan ends earlier).
    last: Level,
}

impl Chain {
    /// Targets for every level `plan` writes.
    pub fn new(gpu: &crate::gpu::Gpu, width: u32, height: u32, plan: &[Pass]) -> Self {
        let mut views = std::collections::HashMap::new();
        let mut textures = Vec::new();
        for p in plan {
            if p.dst == Level::Scene || views.contains_key(&p.dst) {
                continue;
            }
            let (w, h) = level_size(width, height, p.dst);
            let t = crate::post::target(
                gpu,
                "h1_veil",
                w as u32,
                h as u32,
                wgpu::TextureFormat::Rgba16Float,
            );
            views.insert(p.dst, t.create_view(&Default::default()));
            textures.push(t);
        }
        let last = plan.last().map_or(Level::Up(1), |p| p.dst);
        Self { width, height, views, _textures: textures, last }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// What the colour pass reads: the last upsample, `Up(1)`.
    pub fn result(&self) -> Option<&wgpu::TextureView> {
        self.views.get(&self.last)
    }

    /// Records `plan` over `scene`. Each pass reads its source as
    /// `RESOLVED_SCENE` and its second source as `FEEDBACK` (READ from the
    /// zone's `veil_gaussian_add_*` and `tonemap_veil_*` techniques, 3 Nov).
    /// The downsample vertex shaders read `RENDER_TARGET_SIZE` and
    /// `RENDER_SOURCE_SIZE` (INFERRED layout: width, height, 1/width,
    /// 1/height).
    pub fn record(
        &self,
        post: &mut crate::post::Post,
        enc: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        code: &crate::code::CodeValues,
        plan: &[Pass],
    ) -> Result<(), String> {
        let view = |l: Level| -> Result<&wgpu::TextureView, String> {
            if l == Level::Scene {
                Ok(scene)
            } else {
                self.views.get(&l).ok_or_else(|| format!("veil: no target {l:?}"))
            }
        };
        let size4 = |l: Level| {
            let (w, h) = level_size(self.width, self.height, l);
            [w, h, 1.0 / w, 1.0 / h]
        };
        for p in plan {
            let src = view(p.src)?;
            let src2 = match p.src2 {
                Some(l) => view(l)?,
                None => src,
            };
            let mut c = code.clone();
            c.set_vec("VEIL_PARAMS", p.params);
            c.set_vec("RENDER_TARGET_SIZE", size4(p.dst));
            c.set_vec("RENDER_SOURCE_SIZE", size4(p.src));
            c.set_vec("TEXCOORD_CLAMP_1", [0.0, 0.0, 1.0, 1.0]);
            if let Some(w) = p.plain_weights {
                c.set_vec("MATERIAL_COLOR", w);
            }
            let tex: std::collections::HashMap<&str, &wgpu::TextureView> =
                [("RESOLVED_SCENE", src), ("FEEDBACK", src2)].into();
            post.record(
                enc,
                &p.material,
                &tex,
                &c,
                &[],
                view(p.dst)?,
                wgpu::TextureFormat::Rgba16Float,
            )?;
        }
        Ok(())
    }
}

/// The veil colour pass's technique for the tone map in use (the exe
/// picks `tonemap_veil_*` with `_blur` and `_dark` variants; neither of
/// those is used here).
pub fn tonemap_technique(filmic: bool) -> &'static str {
    if filmic { "tonemap_veil_filmic" } else { "tonemap_veil_linear" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_registered_values() {
        let s = Settings::default();
        assert_eq!((s.radius, s.mode, s.quality, s.downsample), (8.0, Mode::Upscale, Quality::Med, Downsample::Log));
        let w = s.normalised_weights();
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!((w[1] / w[0] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn the_blur_step_scales_with_height_and_radius() {
        // 1080 high, radius 8: 18 pixels; a 16:9 view with square pixels.
        let (x, y) = blur_step(8.0, 1920.0, 1080.0, 1.0);
        assert!((y - 18.0).abs() < 1e-4);
        assert!((x - 18.0 * 1920.0 / 1080.0).abs() < 1e-3);
    }

    #[test]
    fn upscale_mode_downsamples_plainly_and_blurs_on_the_way_up() {
        let p = plan(&Settings::default(), 1920, 1080, 1.0, 8);
        assert_eq!(p[0].material, "veil_downsample_log");
        assert_eq!(p[0].dst, Level::Down(0));
        // 8 mips: the first pass, the plain one before the loop and 6 more.
        let downs: Vec<_> = p.iter().take_while(|q| q.material.starts_with("veil_downsample")).collect();
        assert_eq!(downs.len(), 8);
        assert_eq!(downs.last().unwrap().dst, Level::Down(7));
        // Then 6 blurred upsamples of two passes each.
        let up = &p[8..];
        assert_eq!(up.len(), 12);
        assert!(up.iter().all(|q| q.material.starts_with("veil_gaussian")));
        assert_eq!(up[0].material, "veil_gaussian_med");
        assert_eq!(up[1].material, "veil_gaussian_add_med");
    }

    #[test]
    fn the_last_upsample_has_no_second_source_and_the_first_blends_the_smallest_weight() {
        let s = Settings::default();
        let p = plan(&s, 1920, 1080, 1.0, 8);
        let last = p.last().unwrap();
        assert_eq!(last.dst, Level::Up(1));
        assert_eq!(last.src2, None);
        assert_eq!(last.params[2..], [1.0, 1.0][..]);
        // The first pair: the second pass carries (weights[5], weights[4]).
        let w = s.normalised_weights();
        let first_second = &p[9];
        assert_eq!(first_second.src2, Some(Level::Down(6)));
        assert!((first_second.params[2] - w[5]).abs() < 1e-6);
        assert!((first_second.params[3] - w[4]).abs() < 1e-6);
    }

    #[test]
    fn downscale_mode_blurs_down_and_adds_plainly_up() {
        let s = Settings { mode: Mode::Downscale, ..Settings::default() };
        let p = plan(&s, 1280, 720, 1.0, 8);
        assert!(p.iter().any(|q| q.material == "veil_gaussian_med"));
        let adds: Vec<_> = p.iter().filter(|q| q.material == "veil_bilinear_add").collect();
        assert_eq!(adds.len(), 5);
        assert!(p.iter().filter(|q| q.material == "veil_bilinear").count() == 1);
        assert!(adds[0].plain_weights.is_some());
    }

    #[test]
    fn a_gaussian_pair_runs_one_axis_then_the_other_sized_by_its_target() {
        let p = plan(&Settings::default(), 1920, 1080, 1.0, 8);
        let (a, b) = (&p[8], &p[9]);
        assert!(a.params[0] > 0.0 && a.params[1] == 0.0, "x first on the way up");
        assert!(b.params[0] == 0.0 && b.params[1] > 0.0);
        assert!(matches!(a.dst, Level::Temp(_)));
        assert_eq!(b.src, a.dst);
    }

    #[test]
    fn colour_pass_techniques_are_the_exes_material_names() {
        assert_eq!(tonemap_technique(true), "tonemap_veil_filmic");
        assert_eq!(tonemap_technique(false), "tonemap_veil_linear");
    }

    #[test]
    fn target_sizes_follow_the_levels() {
        assert_eq!(level_size(1920, 1080, Level::Down(0)), (960.0, 540.0));
        assert_eq!(level_size(1920, 1080, Level::Down(7)), (7.0, 4.0));
        assert_eq!(level_size(1920, 1080, Level::Up(1)), level_size(1920, 1080, Level::Down(1)));
        assert_eq!(level_size(1920, 1080, Level::Temp(Which::Down(2))), level_size(1920, 1080, Level::Down(3)));
    }

    #[test]
    fn a_short_pyramid_is_the_first_two_passes() {
        let p = plan(&Settings::default(), 640, 360, 1.0, 2);
        assert_eq!(p.len(), 2);
    }
}
