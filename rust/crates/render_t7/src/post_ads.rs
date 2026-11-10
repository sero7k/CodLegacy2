//! The aim-down-sights blur as BlackOps3.exe draws it: Black Ops III's
//! "ADS depth of field" (exe `141d61d40`, `141d60f10`, `141d63100`, in the
//! post frame `141d66110` after the bloom's first two downsamples and
//! before the orchestrator's grade, so the graded image is the blurred one).
//!
//! **When.** The view's DoF block (refdef `+0x38..+0x78`, copied by the view
//! setup `141dddc20` to view `+0xb2634..`): the ADS blur runs while the
//! DoF mode bits (word `+0x38` bits 16..17) are clear and the ADS amount
//! (refdef `+0x68`) is above 0. The client writes that amount every frame
//! (`14118e440`) from the player state's weapon position fraction (cg
//! `+0x11abac`, `ps->fWeaponPosFrac`), zeroed for two pm flags and a
//! client state; `+0x6c` (the fraction where the blur starts) and `+0x70`
//! (above 0.5: the scope variant) come from the weapon (`1405a02a0` via
//! `1427d0ac0`: weapon def `+0x1260` / `+0x125c`, an attachment's
//! `+0x110` / `+0x10c`). The physical camera beside it: focal length from
//! the view's FOV (`1405b2e80`: `7 / tan(atan(tan(fov / 2) · 0.5625))` mm,
//! a 14 mm sensor), focus distance (dvar `0x6c655324`, default 500) and
//! f-stop (dvar `0x7b3e01dc`, default 108, 0.95..120), the scale at
//! `+0x58` (dvar `0x53b683d5`, default 1).
//!
//! **Passes.** With the bool dvar `0x6511eacd` (default 1, the folded path)
//! the bloom's `_l1` downsample is drawn as `bloom_downsample_l1_adsblur`
//! (`_scope` above 0.5; `_scene` variants with SSR): its second target
//! (role `0xb0e42`) is the quarter-size scene with the circle of confusion
//! in alpha, from `downsampledDepth` (render target 0x25, which the depth
//! pre-pass `141d4c3f0` makes for it: `downsample_depth_with_uv_4x_compute`
//! over `floatZ`). Then `141d63100`: `adsblur_large_x` (0xb0e42 → 0xb0e44),
//! `adsblur_large_y` (→ 0xb0e46), `adsblur_small` (0xb0e46 + 0xb0e42 →
//! 0xb0e44), `adsblur_update` (the scene, its depth (code texture 0x1b, the
//! viewmodel band above 0.984375) and 0xb0e44 → a new scene, which the rest
//! of the chain reads). With the dvar off the quarter scene comes from
//! `adsblur_downsample(_scope)` instead (not built: the default folds it).
//!
//! **Constants** (`141d60f10`, `postFxControl0/1`), with `f` the focal
//! length in mm / 25.4 · 0.104286, `H` the frame height:
//! `a = f / ((f / (48 − f) + 1) · fstop)`, `s = −H / 0.551181 · scale`,
//! `k = 1 / (12 H / 1080 − 0.5)`, `c13 = (−a · f · 48 / ((48 − f) ·
//! 0.0015625)) · s · 0.5 · k`, `c8 = (a · 30241 · f / (48 − f)) · s · 0.5 ·
//! k − 0.5 k`; `p = focus − 0.5 · focus` (dvar `0x79589016`, 0.5), at least
//! 1.00049; `control0 = (c13, c8, zNear · 0.984375 / (1 − p), p / (p − 1))`
//! (scope: `(c13, c8, t, 0)`, `t` the amount past the weapon's start
//! fraction, rescaled to 0..1); `control1 = (2/3, c13, c8, amount)`.
//!
//! bo2mp: `BO2MP_T7_ADS_BLUR=0` turns it off (on by default, as the exe);
//! the amount is Black Ops II's `ps.fWeaponPosFrac` (`frame::settings` on
//! the game side); a scoped weapon's blur is left out while its overlay is
//! up (the exe keeps the lens sharp by stencil 128, which Black Ops II's
//! full-screen overlay stands in for); the weapon's start fraction is 0
//! (Black Ops II weapons have none); `BO2MP_T7_ADS_FOCUS`,
//! `BO2MP_T7_ADS_FSTOP` override the dvar defaults.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;

use render_h1::gpu::Gpu;

use crate::compute::{CodeShader, Program, Res};
use crate::gpu::Tex;
use crate::post_pass::{CodeBuffers, CodePass, CodeTech};

/// The passes this blur draws (the bloom `_l1` variants first).
pub const PASSES: [&str; 6] = ["bloom_downsample_l1_adsblur", "bloom_downsample_l1_adsblur_scene", "adsblur_large_x", "adsblur_large_y", "adsblur_small", "adsblur_update"];
const QUARTER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The exe's `PerSceneConsts` registers these passes read (up to
/// `gameTick`, register 70).
const PER_SCENE_REGISTERS: usize = 71;

/// Whether the blur is built (`BO2MP_T7_ADS_BLUR`, default on).
pub fn enabled() -> bool {
    !std::env::var("BO2MP_T7_ADS_BLUR").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

fn env_f32(name: &str) -> Option<f32> {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
}

/// The view's ADS block (refdef `+0x3c..+0x70` as the exe uses it here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdsView {
    /// The ADS amount (refdef `+0x68`, 0..1).
    pub amount: f32,
    /// The weapon's start fraction (refdef `+0x6c`).
    pub start: f32,
    /// The weapon's scope value (refdef `+0x70`; above 0.5 the scope
    /// variant).
    pub scope: f32,
    /// Focal length, mm (refdef `+0x3c`).
    pub focal_mm: f32,
    /// Focus distance (refdef `+0x40`).
    pub focus: f32,
    /// f-stop (refdef `+0x44`).
    pub fstop: f32,
    /// The scale at refdef `+0x58` (dvar default 1).
    pub scale: f32,
}

impl AdsView {
    /// The view at ADS amount `amount` with the exe's dvar defaults and the
    /// focal length from the view's horizontal half-angle tangent `tan_x`.
    pub fn new(amount: f32, tan_x: f32) -> Self {
        Self {
            amount: amount.clamp(0.0, 1.0),
            start: 0.0,
            scope: 0.0,
            focal_mm: focal_length_mm(tan_x),
            focus: env_f32("BO2MP_T7_ADS_FOCUS").unwrap_or(500.0),
            fstop: env_f32("BO2MP_T7_ADS_FSTOP").map_or(108.0, |f| f.clamp(0.95, 120.0)),
            scale: 1.0,
        }
    }

    /// Whether the exe draws the blur for this view (`141d63100`'s gate,
    /// the DoF mode bits being clear here).
    pub fn active(&self) -> bool {
        self.amount > 0.0
    }
}

/// The focal length (mm) the exe makes from the view's FOV (`1405b2e80`):
/// `7 / tan(atan(tan(fov_x / 2) · 0.5625))`, i.e. a 14 mm sensor height
/// over the 16:9 vertical angle. `tan_x`: tan(fov_x / 2).
pub fn focal_length_mm(tan_x: f32) -> f32 {
    let t = f64::from(tan_x.max(1e-4)) * 0.5625;
    (7.0 / ((t.atan() * 2.0) * 0.5).tan()) as f32
}

/// `postFxControl0/1` as `141d60f10` writes them: `near` the view's near
/// plane (view `+0x278`), `height` the frame's height.
pub fn controls(v: &AdsView, near: f32, height: u32) -> [[f32; 4]; 2] {
    let f = (v.focal_mm / 25.4) * 0.104_285_72;
    let f7 = 48.0 - f;
    let a = f / ((f / f7 + 1.0) * v.fstop);
    let h = height as f32;
    let s = -((h / 0.551_181_1) * v.scale);
    let k = 1.0 / ((h * 12.0) / 1080.0 - 0.5);
    let c13 = ((a * -1.0 * f * 48.0) / (f7 * 0.001_562_5)) * s * 0.5 * k;
    let c8 = ((a * 30241.0 * f) / f7) * s * 0.5 * k + k * -0.5;
    let mut p = v.focus - 0.5 * v.focus;
    if p < 1.0 {
        p = 1.000_488_3;
    }
    let zn = near * 0.984_375;
    let mut t = 0.0;
    if v.start <= v.amount {
        t = 1.0;
        if v.amount != 1.0 {
            t = (v.amount - v.start) / (1.0 - v.start);
        }
    }
    let c0 = if v.scope <= 0.5 { [c13, c8, zn / (1.0 - p), p / (p - 1.0)] } else { [c13, c8, t, 0.0] };
    [c0, [2.0 / 3.0, c13, c8, v.amount]]
}

fn per_scene(w: u32, h: u32) -> Vec<u8> {
    let mut r = [[0f32; 4]; PER_SCENE_REGISTERS];
    r[..crate::post_pass::per_scene(w, h).len()].copy_from_slice(&crate::post_pass::per_scene(w, h));
    let (fw, fh) = (w as f32, h as f32);
    // renderTargetSize (code constant 1) and gameTick (4: `.y` 1).
    r[44] = [fw, fh, 1.0 / fw, 1.0 / fh];
    r[70] = [0.0, 1.0, 1.0, 0.0];
    bytemuck::cast_slice(&r).to_vec()
}

/// What a frame's blur reads beside the post chain's own targets.
pub struct Inputs<'a> {
    /// The lit scene (what `bloom_downsample_l0` read).
    pub scene: &'a wgpu::TextureView,
    /// `bloom_downsample_l0`'s output (render target 0x2d).
    pub l0: &'a wgpu::TextureView,
    /// The frame's depth (depth aspect; reversed, the exe's band layout).
    pub depth: &'a wgpu::TextureView,
    /// The quarter size (render target 0x35's).
    pub quarter: (u32, u32),
}

struct Sized {
    width: u32,
    height: u32,
    quarter: (u32, u32),
    float_z: crate::volume_decals::FloatZPass,
    /// downsampledDepth (0x25, ceil-quarter R32F) and its UV (0x27).
    rt25: Tex,
    _rt27: Tex,
    depth_groups: Vec<wgpu::BindGroup>,
    _depth_consts: wgpu::Buffer,
    depth_quarter: [u32; 2],
    /// Roles 0xb0e42, 0xb0e44, 0xb0e46.
    q: [Tex; 3],
    out: Tex,
    /// Per pass (as [`PASSES`]): bind groups, vertices.
    groups: Vec<[wgpu::BindGroup; 3]>,
    vertices: Vec<wgpu::Buffer>,
    _consts: Vec<wgpu::Buffer>,
    key: (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView),
}

pub struct AdsBlur {
    gpu: Gpu,
    passes: Vec<CodePass>,
    depth_program: Program,
    samplers: HashMap<u16, wgpu::Sampler>,
    depth_sampler: wgpu::Sampler,
    buffers: CodeBuffers,
    scene_format: wgpu::TextureFormat,
    /// `PostFxCBuffer` of the passes: the bloom constants with this view's
    /// `postFxControl0/1` (written by the post chain each frame).
    pub fx: wgpu::Buffer,
    sized: Option<Sized>,
}

impl AdsBlur {
    /// `techs`: core_patch's code techniques ([`PASSES`]); `shaders`: its
    /// compute shaders; `scene_format`: the lit scene's.
    pub fn new(gpu: &Gpu, techs: &HashMap<String, CodeTech>, shaders: &HashMap<String, CodeShader>, scene_format: wgpu::TextureFormat) -> Result<Self, String> {
        let mut passes = Vec::new();
        for n in PASSES {
            let t = techs.get(n).ok_or_else(|| format!("{n} not in core_patch"))?;
            let formats: Vec<wgpu::TextureFormat> = match n {
                "bloom_downsample_l1_adsblur" => vec![wgpu::TextureFormat::Rgba16Float, QUARTER_FORMAT],
                "bloom_downsample_l1_adsblur_scene" => vec![wgpu::TextureFormat::Rgba16Float, QUARTER_FORMAT, wgpu::TextureFormat::Rgba16Float],
                "adsblur_update" => vec![scene_format],
                _ => vec![QUARTER_FORMAT],
            };
            // (Loaded, not sampled: `downsampledDepth` (0x1b of the
            // downsample) and the frame's depth (0x1b of the update).)
            let unfilterable: &[u32] = if n == "adsblur_small" { &[] } else { &[0x1b] };
            passes.push(CodePass::new(gpu, t, &formats, unfilterable)?);
        }
        let cs = shaders.get("downsample_depth_with_uv_4x_compute").ok_or("downsample_depth_with_uv_4x_compute not in core_patch")?;
        let depth_program = Program::new_opts(gpu, cs, &[(0, "r32float"), (1, "r32uint")], &[0], true)?;
        let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let mut samplers = HashMap::new();
        for p in &passes {
            for s in p.sampler_states() {
                if let std::collections::hash_map::Entry::Vacant(e) = samplers.entry(s) {
                    e.insert(gpu.device.create_sampler(&crate::sampler::descriptor(s, crate::sampler::ANISO_QUALITY, border)?));
                }
            }
        }
        let depth_sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("t7 ads depthSampler"), ..Default::default() });
        let fx = crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some("t7 ads PostFxCBuffer"), size: crate::post::PostFxConsts::BYTES, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Ok(Self { gpu: gpu.clone(), passes, depth_program, samplers, depth_sampler, buffers: CodeBuffers::new(gpu), scene_format, fx, sized: None })
    }

    fn make_sized(&self, w: u32, h: u32, i: &Inputs) -> Result<Sized, String> {
        use wgpu::util::DeviceExt;
        let gpu = &self.gpu;
        let dev = &gpu.device;
        let (qw, qh) = i.quarter;
        let float_z = crate::volume_decals::FloatZPass::new(gpu, w, h, i.depth);
        // The depth pre-pass's targets (the exe's 0x25 / 0x27, ceil quarter).
        let (dw, dh) = crate::volumetric::quarter(w, h);
        let storage = |label: &str, format| {
            let texture = crate::audit::texture(gpu, &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: dw.max(1), height: dh.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            Tex { texture, view }
        };
        let rt25 = storage("t7 ads downsampledDepth (0x25)", wgpu::TextureFormat::R32Float);
        let rt27 = storage("t7 ads downSampledDepthUV (0x27)", wgpu::TextureFormat::R32Uint);
        let mut dc = [[0f32; 4]; 17];
        dc[7] = [4.0, 0.0, 0.0, 0.0];
        dc[16] = [1.0 / w as f32, 1.0 / h as f32, dw as f32, dh as f32];
        let depth_consts = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 ads depth PostFxCBuffer"), contents: bytemuck::cast_slice(&dc), usage: wgpu::BufferUsages::UNIFORM });
        let depth_groups = self.depth_program.bind(gpu, |s| match s.name.as_str() {
            "PostFxCBuffer" => Some(Res::Buffer(&depth_consts)),
            "floatZSampler" => Some(Res::View(&float_z.out.view)),
            "depthSampler" => Some(Res::Sampler(&self.depth_sampler)),
            "postEffect1" => Some(Res::View(&rt25.view)),
            "postEffect2" => Some(Res::View(&rt27.view)),
            _ => None,
        })?;
        let q = [
            crate::gpu::target(gpu, "t7 ads quarter scene + CoC (role 0xb0e42)", qw, qh, QUARTER_FORMAT, wgpu::TextureUsages::COPY_SRC),
            crate::gpu::target(gpu, "t7 ads blur (role 0xb0e44)", qw, qh, QUARTER_FORMAT, wgpu::TextureUsages::COPY_SRC),
            crate::gpu::target(gpu, "t7 ads blur (role 0xb0e46)", qw, qh, QUARTER_FORMAT, wgpu::TextureUsages::COPY_SRC),
        ];
        let out = crate::gpu::target(gpu, "t7 ads scene (role 0xb0e4a)", w, h, self.scene_format, wgpu::TextureUsages::COPY_SRC);
        let mut consts = Vec::new();
        let mut groups = Vec::new();
        let mut vertices = Vec::new();
        for (k, p) in self.passes.iter().enumerate() {
            let (pw, ph) = if k == 5 { (w, h) } else { (qw, qh) };
            let ps = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 ads PerSceneConsts"), contents: &per_scene(pw, ph), usage: wgpu::BufferUsages::UNIFORM });
            let tex = |id: u32| -> Option<&wgpu::TextureView> {
                match (k, id) {
                    (0 | 1, 0x1a) => Some(i.l0),
                    (0 | 1, 0x1b) => Some(&rt25.view),
                    (2, 0x1a) => Some(&q[0].view),
                    (3, 0x1a) => Some(&q[1].view),
                    (4, 0x1a) => Some(&q[2].view),
                    (4, 0x1b) => Some(&q[0].view),
                    (5, 0x1a) => Some(i.scene),
                    (5, 0x1b) => Some(i.depth),
                    (5, 0x1c) => Some(&q[1].view),
                    _ => None,
                }
            };
            groups.push(p.bind(gpu, &ps, &self.fx, &self.buffers, tex, |s| self.samplers.get(&s))?);
            vertices.push(crate::gpu::buffer(gpu, "t7 ads triangle", &crate::post_pass::triangle(pw, ph), wgpu::BufferUsages::VERTEX));
            consts.push(ps);
        }
        Ok(Sized {
            width: w,
            height: h,
            quarter: i.quarter,
            float_z,
            rt25,
            _rt27: rt27,
            depth_groups,
            _depth_consts: depth_consts,
            depth_quarter: [dw, dh],
            q,
            out,
            groups,
            vertices,
            _consts: consts,
            key: (i.scene.clone(), i.l0.clone(), i.depth.clone()),
        })
    }

    /// Makes (or keeps) the targets and bindings for a `w` x `h` frame.
    pub fn prepare(&mut self, w: u32, h: u32, i: &Inputs) -> Result<(), String> {
        if self.sized.as_ref().is_none_or(|s| s.width != w || s.height != h || s.quarter != i.quarter || s.key.0 != *i.scene || s.key.1 != *i.l0 || s.key.2 != *i.depth) {
            self.sized = Some(self.make_sized(w, h, i)?);
        }
        Ok(())
    }

    /// The blurred scene (valid after [`Self::prepare`]).
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.out.view)
    }

    /// Records the depth pre-pass (`floatZ`, then the exe's
    /// `downsample_depth_with_uv_4x_compute` into 0x25) and the bloom's
    /// `_l1` as `bloom_downsample_l1_adsblur` (`targets`: render target
    /// 0x35, and with SSR `gDownsampledScene`: the `_scene` variant).
    pub fn record_l1(&self, enc: &mut wgpu::CommandEncoder, near: f32, l1: &wgpu::TextureView, ssr_scene: Option<&wgpu::TextureView>) {
        let Some(s) = self.sized.as_ref() else { return };
        s.float_z.set_near(&self.gpu.queue, near);
        s.float_z.record(enc);
        {
            let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 ads downsample_depth_with_uv_4x_compute"), timestamp_writes: None });
            pass.set_pipeline_p(&self.depth_program.pipeline);
            for (k, g) in s.depth_groups.iter().enumerate() {
                pass.set_bind_group_p(k as u32, g, &[]);
            }
            let [x, y, z] = self.depth_program.groups_for([s.depth_quarter[0], s.depth_quarter[1], 1]);
            pass.dispatch_p(x, y, z);
        }
        match ssr_scene {
            Some(down) => self.passes[1].draw(enc, &[l1, &s.q[0].view, down], &s.groups[1], &s.vertices[1]),
            None => self.passes[0].draw(enc, &[l1, &s.q[0].view], &s.groups[0], &s.vertices[0]),
        }
    }

    /// Records `adsblur_large_x`, `_large_y`, `_small` and `_update`
    /// (exe `141d63100`); returns the blurred scene.
    pub fn record_blur(&self, enc: &mut wgpu::CommandEncoder) -> Option<&wgpu::TextureView> {
        let s = self.sized.as_ref()?;
        let targets = [&s.q[1].view, &s.q[2].view, &s.q[1].view, &s.out.view];
        for (k, t) in targets.iter().enumerate() {
            self.passes[k + 2].draw(enc, &[t], &s.groups[k + 2], &s.vertices[k + 2]);
        }
        Some(&s.out.view)
    }

    /// The targets of the last run (offline checks): downsampledDepth, the
    /// quarter scene + CoC, the two blurs and the output.
    pub fn targets(&self) -> Option<[&wgpu::Texture; 5]> {
        let s = self.sized.as_ref()?;
        Some([&s.rt25.texture, &s.q[0].texture, &s.q[1].texture, &s.q[2].texture, &s.out.texture])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focal_length_from_the_fov() {
        // 65 degrees across: 7 / (tan 32.5 · 0.5625) mm.
        let t = (32.5f32).to_radians().tan();
        let f = focal_length_mm(t);
        assert!((f - 7.0 / (t * 0.5625)).abs() < 1e-3, "{f}");
        assert!((f - 19.53).abs() < 0.05, "{f}");
    }

    #[test]
    fn controls_follow_the_exe() {
        let v = AdsView { amount: 1.0, start: 0.0, scope: 0.0, focal_mm: 19.53, focus: 500.0, fstop: 108.0, scale: 1.0 };
        let [c0, c1] = controls(&v, 4.0, 1080);
        // The near term's two coefficients: 30720 : −30241 (zero at depth
        // 0.98441, the viewmodel band's start), less 0.5 k.
        let k = 1.0 / 11.5;
        assert!(c0[0] > 0.0 && c0[1] < 0.0, "{c0:?}");
        assert!(((c0[1] + 0.5 * k) / c0[0] + 30241.0 / 30720.0).abs() < 1e-4, "{c0:?}");
        // The far term: zNear·0.984375 / (1 − 250), 250 / 249.
        assert!((c0[2] - 4.0 * 0.984375 / (1.0 - 250.0)).abs() < 1e-6);
        assert!((c0[3] - 250.0 / 249.0).abs() < 1e-6);
        assert_eq!(c1[0], 2.0 / 3.0);
        assert_eq!((c1[1], c1[2], c1[3]), (c0[0], c0[1], 1.0));
        // The world blurs in front of ~62 units (zNear 4): CoC
        // `sat(c0.z · z + c0.w) · 2/3` with z the linear depth.
        let coc = |z: f32| (c0[2] * z + c0[3]).clamp(0.0, 1.0) * c1[0];
        assert!(coc(10.0) > 0.5 && coc(100.0) == 0.0);
        // Scope: the amount past the start fraction.
        let s = AdsView { scope: 1.0, start: 0.5, amount: 0.75, ..v };
        let [c0, c1] = controls(&s, 4.0, 1080);
        assert_eq!((c0[2], c0[3], c1[3]), (0.5, 0.0, 0.75));
        // Below the start: none.
        let s = AdsView { amount: 0.25, ..s };
        assert_eq!(controls(&s, 4.0, 1080)[0][2], 0.0);
    }

    fn read_rgba16f(gpu: &Gpu, t: &wgpu::Texture) -> Vec<[f32; 4]> {
        let (w, h) = (t.width(), t.height());
        let row = (w * 8).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, t.size());
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let m = buf.slice(..).get_mapped_range();
        let h16 = |b: &[u8]| {
            let h = u16::from_le_bytes([b[0], b[1]]);
            let e = ((h >> 10) & 0x1f) as i32;
            let f = f32::from(h & 0x3ff);
            let v = if e == 0 { f / 1024.0 * 2f32.powi(-14) } else { (1.0 + f / 1024.0) * 2f32.powi(e - 15) };
            if h & 0x8000 != 0 { -v } else { v }
        };
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let o = (y * row + x * 8) as usize;
                out.push(std::array::from_fn(|c| h16(&m[o + 2 * c..])));
            }
        }
        out
    }

    /// The exe's passes over a synthetic 128x64 frame: a checkerboard
    /// scene, the left half's depth 10 units away, the right half's 1000.
    /// At full ADS the near half is blurred (its checkerboard's contrast
    /// falls), the far half kept; at ADS 0 the update leaves the scene as
    /// it was.
    #[test]
    fn ads_blur_softens_the_near_half_only() {
        let dir = std::path::PathBuf::from(std::env::var("T7_ZONE_DIR").unwrap_or_else(|_| "/mnt/user-data/uploads/zone".into()));
        if !dir.join("core_patch.ff").exists() {
            return eprintln!("skipped: no core_patch.ff");
        }
        let Ok(gpu) = Gpu::headless() else { return eprintln!("skipped: no device") };
        let patch = fastfile_t7::shared::zone(&dir, "core_patch").expect("core_patch");
        let techs = crate::post_pass::code_techs(&patch.image, &patch.capture, &PASSES).expect("techs");
        let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
        let fmt = wgpu::TextureFormat::Rgba16Float;
        let mut ads = AdsBlur::new(&gpu, &techs, &shaders, fmt).expect("ads blur");
        let (w, h) = (128u32, 64u32);
        let (qw, qh) = (w / 4, h / 4);
        let near = 4.0f32;
        let scene = crate::gpu::target(&gpu, "scene", w, h, fmt, wgpu::TextureUsages::COPY_DST);
        let l0 = crate::gpu::target(&gpu, "l0", w / 2, h / 2, fmt, wgpu::TextureUsages::COPY_DST);
        let l1 = crate::gpu::target(&gpu, "l1", qw, qh, fmt, wgpu::TextureUsages::empty());
        let depth = crate::gpu::target(&gpu, "depth", w, h, crate::gbuffer::DEPTH, wgpu::TextureUsages::empty());
        let depth_view = depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        // A checkerboard of 4-pixel squares (0.1 / 1.0), in the scene and
        // at half size in l0.
        let fill = |t: &Tex, tw: u32, th: u32, sq: u32| {
            let mut px = Vec::new();
            for y in 0..th {
                for x in 0..tw {
                    let v: u16 = if (x / sq + y / sq) % 2 == 0 { 0x3c00 } else { 0x2e66 };
                    for c in [v, v, v, 0x3c00] {
                        px.extend_from_slice(&c.to_le_bytes());
                    }
                }
            }
            gpu.queue.write_texture_p(t.texture.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tw * 8), rows_per_image: None }, t.texture.size());
        };
        fill(&scene, w, h, 4);
        fill(&l0, w / 2, h / 2, 2);
        // The depth (reversed, `near / z`): z 10 on the left, 1000 on the
        // right, written by a full-screen draw.
        {
            let src = format!(
                "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{ let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)); return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0); }}
                 struct O {{ @builtin(frag_depth) d: f32 }}
                 @fragment fn fs(@builtin(position) p: vec4<f32>) -> O {{ var o: O; o.d = select({far}, {nearz}, p.x < {half}.0); return o; }}",
                far = near / 1000.0,
                nearz = near / 10.0,
                half = w / 2
            );
            let m = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
            let pl = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState { module: &m, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState { format: crate::gbuffer::DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Always), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &m, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[] }),
                multiview_mask: None,
                cache: crate::pipecache::get(&gpu.device).as_ref(),
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut p = enc.render_pass_p(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth.view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }) }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                p.set_pipeline_p(&pl);
                p.draw_p(0..3, 0..1);
            }
            gpu.queue.submit([enc.finish()]);
        }
        let mut run = |amount: f32| -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
            let inputs = Inputs { scene: &scene.view, l0: &l0.view, depth: &depth_view, quarter: (qw, qh) };
            ads.prepare(w, h, &inputs).expect("prepare");
            let v = AdsView { amount, start: 0.0, scope: 0.0, focal_mm: focal_length_mm(1.0), focus: 500.0, fstop: 108.0, scale: 1.0 };
            let mut c = crate::post::PostFxConsts::default();
            let [c0, c1] = controls(&v, near, h);
            c.control(0, c0);
            c.control(1, c1);
            gpu.queue.write_buffer_p(&ads.fx, 0, &c.bytes());
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            ads.record_l1(&mut enc, near, &l1.view, None);
            ads.record_blur(&mut enc).expect("blur");
            gpu.queue.submit([enc.finish()]);
            assert!(pollster::block_on(scope.pop()).is_none(), "validation error");
            let t = ads.targets().unwrap();
            (read_rgba16f(&gpu, t[1]), read_rgba16f(&gpu, t[4]))
        };
        let contrast = |img: &[[f32; 4]], x0: u32, x1: u32| -> f32 {
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for y in 16..48 {
                for x in x0..x1 {
                    let v = img[(y * w + x) as usize][0];
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            hi - lo
        };
        let (_, still) = run(0.0);
        let (base_l, base_r) = (contrast(&still, 8, 56), contrast(&still, 72, 120));
        eprintln!("ADS 0: contrast left {base_l}, right {base_r}");
        assert!(base_l > 0.8 && base_r > 0.8, "no ADS: the scene kept");
        let (quarter, aimed) = run(1.0);
        let coc: Vec<f32> = (0..qw).map(|x| quarter[(qh / 2 * qw + x) as usize][3]).collect();
        eprintln!("ADS 1: quarter CoC row {coc:?}");
        let (l, r) = (contrast(&aimed, 8, 56), contrast(&aimed, 72, 120));
        eprintln!("ADS 1: contrast left {l}, right {r}");
        assert!(coc[2] > 0.3 && coc[(qw - 3) as usize] < 0.05, "CoC: near half blurred, far half not");
        assert!(l < 0.6 * base_l, "the near half blurred ({l} of {base_l})");
        assert!(r > 0.9 * base_r, "the far half kept ({r} of {base_r})");
    }
}
