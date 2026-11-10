//! Ambient occlusion as the exe draws it (`0x141d6de80`, between the motion
//! vectors and the deferred lighting in `0x141d5ca10`): GTAO at half
//! resolution into render target 0x1e, then a denoiser into the frame's
//! slot of the 0x1b..0x1d ring (`(frame + 2) % 3`), which the lighting reads
//! as `gSSAOTexture` (code texture 0x55, `0x141d6dd20`).
//!
//! The pass runs when the quality dvar (`_DAT_1486bf5a0`, an enum: 0
//! Disabled, 1 HEMIAO, 2..5 GTAO Low/Medium/High/Ultra; default 3) is 2..5
//! and the view is not a no-AO view; otherwise the lighting reads render
//! target 0x19, cleared to 1. With one GPU (`_DAT_1486f0ef0`, the GPU
//! count, 1) the dvar picks `ssao_gtao_{low,med,high}` (dvar − 2) with a
//! temporal denoiser, `_t2x_{1,2}` when the anti-aliasing mode
//! (`0x141d634d0`; dvar `_DAT_1486bf4d0`, default 5 Filmic SMAA T2x) is 4
//! or 5, else `_1x_{1,2}`, `_2` on even frames; Ultra (5), or more than one
//! GPU, is `ssao_gtao_ultra` with `ssao_denoiser_spatial`.
//!
//! Code textures (code texture n is bound by `0x1150 + 8n` in the draw
//! state): GTAO reads `GBufferNormalAndGloss` (0x31) and the frame's depth
//! pyramid (0x1c: the ring image the uber resolve wrote, mips DS2x..DS64x);
//! the denoiser reads the depth pyramids of this and the two previous
//! frames (0x1a, 0x1b, 0x1c), the motion vectors of this and the previous
//! frame (0x1d, 0x1e: render targets 0x16/0x17 by frame parity), the raw AO
//! (0x1f = render target 0x1e) and the two previous AO results (0x20, 0x21).
//! Code constants: `postFxControl0` (0x33) = `(h/w / t, 1 / t, rotation,
//! offset)` with `t` the view's tan(fov_y / 2) and h/w render target 0x1e's,
//! rotation `{60, 300, 180, 240, 120, 0}[frame % 6] / 360` and offset `{0,
//! 0.5, 0.25, 0.75}[(frame / 6) & 3]` on the temporal path (0 otherwise);
//! `postFxControl1` (0x34) = `(exp(−dt/75), exp(−dt/215), 0, 0)`, dt the
//! frame time in milliseconds. `viewSpaceScaleBias` (0x6) is the view
//! setup's (`0x141dddc20`): `(2·a·t, −2·t, a·t, −t)`, a the view's aspect.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;

use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::gpu::Tex;
use crate::post_pass::{code_techs, per_scene, triangle, CodeBuffers, CodePass, CodeTech};

/// The exe's default for the quality dvar (GTAO Medium Quality).
pub const QUALITY_DEFAULT: i32 = 3;
/// The quality run when no override is given: the exe's default (its
/// temporal denoisers read the motion vectors `crate::motion` makes; the
/// lighting falls back to Ultra, 5, if they can't be made).
pub const QUALITY_RUN: i32 = QUALITY_DEFAULT;
/// The exe's default anti-aliasing mode (Filmic SMAA T2x).
pub const AA_MODE_DEFAULT: i32 = 5;
const GTAO: [&str; 4] = ["ssao_gtao_low", "ssao_gtao_med", "ssao_gtao_high", "ssao_gtao_ultra"];
const ROTATION: [f32; 6] = [60.0, 300.0, 180.0, 240.0, 120.0, 0.0];
const OFFSET: [f32; 4] = [0.0, 0.5, 0.25, 0.75];
/// R8_UNORM targets (0x1b..0x1e).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// `PerSceneConsts` registers the AO programs read (up to
/// `viewSpaceScaleBias` at 73).
const REGISTERS: usize = 75;

/// What the exe would run for a quality and anti-aliasing mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub gtao: &'static str,
    /// The denoisers, by frame parity: `[odd, even]` (one for the spatial
    /// denoiser).
    pub denoisers: Vec<&'static str>,
    pub temporal: bool,
}

/// `0x141d5ca10` / `0x141d6de80` with one GPU: `None` when the pass does
/// not run.
pub fn choose(quality: i32, aa_mode: i32, gpus: i32) -> Option<Choice> {
    if !(2..=5).contains(&quality) {
        return None;
    }
    let temporal = gpus == 1 && quality != 5;
    if !temporal {
        return Some(Choice { gtao: GTAO[3], denoisers: vec!["ssao_denoiser_spatial"], temporal });
    }
    let t2x = aa_mode == 4 || aa_mode == 5;
    let d = if t2x {
        vec!["ssao_denoiser_spatial_temporal_t2x_1", "ssao_denoiser_spatial_temporal_t2x_2"]
    } else {
        vec!["ssao_denoiser_spatial_temporal_1x_1", "ssao_denoiser_spatial_temporal_1x_2"]
    };
    Some(Choice { gtao: GTAO[(quality - 2) as usize], denoisers: d, temporal })
}

/// The dvar values in effect: `T7_SSAO_QUALITY` / `T7_AA_MODE` override
/// (as a config would) [`QUALITY_RUN`] and the AA mode this renderer runs
/// (`crate::post_aa::effective` of the asked mode: SMAA 1x for the exe's
/// default T2x, whose jittered temporal resolve is not drawn). The exe picks
/// the `_t2x` denoisers only with T2x running, whose resolve blends their
/// alternating halves; without it they flickered frame to frame.
pub fn dvars() -> (i32, i32) {
    let get = |k: &str, d: i32| {
        codlegacy_env::var(k)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(d)
    };
    (
        get("T7_SSAO_QUALITY", crate::quality::get().ssao),
        get(
            "T7_AA_MODE",
            crate::post_aa::effective(crate::post_aa::mode()),
        ),
    )
}

/// The AO targets for a frame size.
struct Sized {
    width: u32,
    height: u32,
    /// Render target 0x1e (GTAO's output).
    raw: Tex,
    /// Render targets 0x1b..0x1d (the denoised ring).
    ring: [Tex; 3],
    /// GTAO's bind groups per depth-ring slot.
    gtao: Vec<[wgpu::BindGroup; 3]>,
    /// Per denoiser (by parity), per ring slot.
    denoise: Vec<Vec<[wgpu::BindGroup; 3]>>,
}

pub struct Ao {
    gpu: Gpu,
    pub choice: Choice,
    gtao: CodePass,
    denoisers: Vec<CodePass>,
    args: Vec<fastfile_t7::walk::capture::ShaderArg>,
    buffers: CodeBuffers,
    samplers: HashMap<u16, wgpu::Sampler>,
    per_scene: wgpu::Buffer,
    post_fx: wgpu::Buffer,
    vertices: Option<(u32, u32, wgpu::Buffer)>,
    sized: Option<Sized>,
    last_ms: Option<i32>,
}

fn target(gpu: &Gpu, label: &str, w: u32, h: u32) -> Tex {
    crate::gpu::target(gpu, label, w, h, FORMAT, wgpu::TextureUsages::COPY_SRC)
}

impl Ao {
    /// The passes for `choice` from `core_patch`; `Err` when the temporal
    /// path's inputs are not all made (motion vectors).
    pub fn new(gpu: &Gpu, image: &[u8], cap: &fastfile_t7::walk::capture::Capture, choice: Choice, motion_vectors: bool) -> Result<Self, String> {
        if choice.temporal && !motion_vectors {
            return Err(format!("{} needs the motion vectors (render targets 0x16/0x17), not made yet", choice.denoisers[0]));
        }
        let mut names = vec![choice.gtao];
        names.extend(choice.denoisers.iter().copied());
        let techs: HashMap<String, CodeTech> = code_techs(image, cap, &names)?;
        let gtao = CodePass::new(gpu, &techs[choice.gtao], &[FORMAT], &[])?;
        let denoisers = choice.denoisers.iter().map(|n| CodePass::new(gpu, &techs[*n], &[FORMAT], &[])).collect::<Result<Vec<_>, _>>()?;
        let mut args = Vec::new();
        for t in techs.values() {
            args.extend(t.args.iter().cloned());
        }
        let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let mut samplers = HashMap::new();
        for p in std::iter::once(&gtao).chain(denoisers.iter()) {
            for s in p.sampler_states() {
                if let std::collections::hash_map::Entry::Vacant(e) = samplers.entry(s) {
                    e.insert(gpu.device.create_sampler(&crate::sampler::descriptor(s, crate::sampler::ANISO_QUALITY, border)?));
                }
            }
        }
        let u = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let mk = |label: &str, size: u64| crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some(label), size, usage: u, mapped_at_creation: false });
        Ok(Self {
            gpu: gpu.clone(),
            choice,
            gtao,
            denoisers,
            args,
            buffers: CodeBuffers::new(gpu),
            samplers,
            per_scene: mk("t7 ao PerSceneConsts", (REGISTERS * 16) as u64),
            post_fx: mk("t7 ao PostFxCBuffer", 32),
            vertices: None,
            sized: None,
            last_ms: None,
        })
    }

    /// The half-resolution size the targets take for a `w` × `h` view (the
    /// depth pyramid's first level).
    pub fn half(w: u32, h: u32) -> (u32, u32) {
        (w.div_ceil(2).max(1), h.div_ceil(2).max(1))
    }

    /// Makes the targets and bind groups: `normal` the G-buffer's
    /// `GBufferNormalAndGloss`, `depth[r]` the depth-ring slot `r` (all
    /// mips).
    /// `velocity`: the motion-vector targets by frame parity (render
    /// targets 0x16/0x17), which the temporal denoisers read as this
    /// frame's (0x1d) and the previous frame's (0x1e).
    pub fn prepare(&mut self, w: u32, h: u32, normal: &wgpu::TextureView, depth: [&wgpu::TextureView; 3], velocity: Option<[&wgpu::TextureView; 2]>) -> Result<(), String> {
        let gpu = &self.gpu;
        let (hw, hh) = Self::half(w, h);
        let raw = target(gpu, "ssao raw (rt 0x1e)", hw, hh);
        let ring = std::array::from_fn(|i| target(gpu, &format!("ssao (rt {:#x})", 0x1b + i), hw, hh));
        let samplers = &self.samplers;
        let mut gtao = Vec::new();
        let mut denoise: Vec<Vec<[wgpu::BindGroup; 3]>> = vec![Vec::new(); self.denoisers.len()];
        for r in 0..3 {
            // r is the frame's slot ((f + 2) % 3): the previous frame's is
            // (r + 2) % 3, the one before (r + 1) % 3.
            let tex = |id: u32| -> Option<&wgpu::TextureView> {
                Some(match id {
                    0x31 => normal,
                    0x1c => depth[r],
                    _ => return None,
                })
            };
            gtao.push(self.gtao.bind(gpu, &self.per_scene, &self.post_fx, &self.buffers, tex, |s| samplers.get(&s))?);
        }
        // The denoisers by frame mod 6 (the depth/AO ring of 3 and the
        // motion-vector pair by parity).
        for f in 0..6usize {
            let r = (f + 2) % 3;
            let (p1, p2) = ((r + 2) % 3, (r + 1) % 3);
            for (k, d) in self.denoisers.iter().enumerate() {
                let tex = |id: u32| -> Option<&wgpu::TextureView> {
                    Some(match id {
                        0x1a => depth[r],
                        0x1b => depth[p1],
                        0x1c => depth[p2],
                        0x1d => velocity?[f & 1],
                        0x1e => velocity?[(f + 1) & 1],
                        0x1f => &raw.view,
                        0x20 => &ring[p1].view,
                        0x21 => &ring[p2].view,
                        _ => return None,
                    })
                };
                denoise[k].push(d.bind(gpu, &self.per_scene, &self.post_fx, &self.buffers, tex, |s| samplers.get(&s))?);
            }
        }
        self.vertices = Some((hw, hh, crate::gpu::buffer(gpu, "ssao triangle", &triangle(hw, hh), wgpu::BufferUsages::VERTEX)));
        self.sized = Some(Sized { width: hw, height: hh, raw, ring, gtao, denoise });
        Ok(())
    }

    /// The AO the lighting reads this frame (`gSSAOTexture`) for ring slot
    /// `r`.
    pub fn output(&self, r: usize) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.ring[r].view)
    }

    /// The AO texture of ring slot `r` and the raw GTAO target (for
    /// checks).
    pub fn textures(&self, r: usize) -> Option<(&wgpu::Texture, &wgpu::Texture)> {
        self.sized.as_ref().map(|s| (&s.ring[r].texture, &s.raw.texture))
    }

    /// The frame's constants: `view_matrix` the view's `viewMatrix` rows,
    /// `aspect` the view's, `now` milliseconds, `frame` the frame counter.
    pub fn update(&mut self, camera: &Camera, view_matrix: &[[f32; 4]], aspect: f32, now: i32, frame: u64) {
        let Some(s) = self.sized.as_ref() else { return };
        let (w, h) = (s.width as f32, s.height as f32);
        let mut regs = [[0f32; 4]; REGISTERS];
        regs[..25].copy_from_slice(&per_scene(s.width, s.height));
        let mut consts = crate::code::CodeConsts::default();
        consts.set(crate::lighting_globals::RENDER_TARGET_SIZE, vec![[w, h, 1.0 / w, 1.0 / h]]);
        let t = camera.tan_y;
        consts.set(VIEW_SPACE_SCALE_BIAS, vec![[2.0 * aspect * t, -2.0 * t, aspect * t, -t]]);
        consts.set(crate::lighting_globals::VIEW_MATRIX, view_matrix.to_vec());
        let mut bytes = bytemuck::cast_slice::<[f32; 4], u8>(&regs).to_vec();
        let mut missing = Vec::new();
        crate::lighting::fill_code(&mut bytes, &self.args, &consts, &mut missing);
        self.gpu.queue.write_buffer_p(&self.per_scene, 0, &bytes);
        let (rot, off) = if self.choice.temporal { (ROTATION[(frame % 6) as usize] / 360.0, OFFSET[((frame / 6) & 3) as usize]) } else { (0.0, 0.0) };
        let dt = self.last_ms.map_or(0.0, |l| (now - l).max(0) as f32);
        self.last_ms = Some(now);
        let fx = [[(h / w) / t, 1.0 / t, rot, off], [(-dt / 75.0).exp(), (-dt / 215.0).exp(), 0.0, 0.0]];
        self.gpu.queue.write_buffer_p(&self.post_fx, 0, bytemuck::cast_slice(&fx));
    }

    /// Records GTAO and the denoiser for frame `frame` (depth ring slot
    /// `(frame + 2) % 3`).
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, frame: u64) {
        let (Some(s), Some((_, _, v))) = (self.sized.as_ref(), self.vertices.as_ref()) else { return };
        let r = ((frame + 2) % 3) as usize;
        self.gtao.draw(enc, &[&s.raw.view], &s.gtao[r], v);
        // `_1` on odd frames, `_2` on even (0x141d6de80).
        let k = if self.denoisers.len() == 2 && frame & 1 == 0 { 1 } else { 0 };
        self.denoisers[k].draw(enc, &[&s.ring[r].view], &s.denoise[k][(frame % 6) as usize], v);
    }
}

/// `viewSpaceScaleBias`.
pub const VIEW_SPACE_SCALE_BIAS: u16 = 0x6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices() {
        assert_eq!(choose(0, 5, 1), None);
        assert_eq!(choose(1, 5, 1), None);
        let c = choose(3, 5, 1).unwrap();
        assert_eq!(c.gtao, "ssao_gtao_med");
        assert!(c.temporal);
        assert_eq!(c.denoisers[0], "ssao_denoiser_spatial_temporal_t2x_1");
        assert_eq!(choose(2, 2, 1).unwrap().denoisers[1], "ssao_denoiser_spatial_temporal_1x_2");
        let u = choose(5, 5, 1).unwrap();
        assert_eq!((u.gtao, u.temporal), ("ssao_gtao_ultra", false));
        assert_eq!(choose(3, 5, 2).unwrap().gtao, "ssao_gtao_ultra");
        // The exe's default T2x runs here as SMAA 1x: the 1x denoisers.
        assert_eq!(choose(3, crate::post_aa::effective(AA_MODE_DEFAULT), 1).unwrap().denoisers[0], "ssao_denoiser_spatial_temporal_1x_1");
    }

    /// The 1x denoisers (what runs with this renderer's SMAA 1x) build from
    /// core_patch on a device like the T2x ones did.
    #[test]
    #[ignore = "needs the staged core_patch.ff (T7_ZONE_DIR)"]
    fn one_x_denoisers_build() {
        let Ok(dir) = std::env::var("T7_ZONE_DIR") else { return eprintln!("skipped: no T7_ZONE_DIR") };
        let Ok(gpu) = Gpu::headless() else { return eprintln!("skipped: no device") };
        let patch = fastfile_t7::shared::zone(std::path::Path::new(&dir), "core_patch").expect("core_patch");
        for aa in [2, 5] {
            let c = choose(3, aa, 1).unwrap();
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            Ao::new(&gpu, &patch.image, &patch.capture, c.clone(), true).unwrap_or_else(|e| panic!("{c:?}: {e}"));
            assert!(pollster::block_on(scope.pop()).is_none(), "{c:?}: validation");
        }
    }
}
