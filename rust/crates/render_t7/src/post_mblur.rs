//! Camera motion blur as BlackOps3.exe draws it (exe `0x141d66110`, first
//! thing of the post frame, before the bloom reads the lit scene).
//!
//! Whether a view blurs is worked out by `0x141ddc6c0` (the view setup's
//! motion-vector parameters) into the view state `S+0x1d30`: byte 1 the
//! blur flag, the float at +4 its scale (`MotionVectorParams` 12.w). The
//! motion blur dvar (hash `0x48c1287c`) is an enum Off / Auto / On,
//! registered default 1 (Auto); its scale dvar (`0x75eb6bca`) defaults to
//! 1. On: blur at that scale. Auto: the scale times the refdef's blur
//! amount (refdef `+0x74`, cg `+0x131d64`: 0 at map start, set only by a
//! script command, `140304960`, clamped to 0..1), on when above 1e-6; a
//! client flag (bit 30 of `+0xd5f48`) forces it with a set amount (1,
//! 0.125, 0.625, 0.25, 0.75, 0.375 or 0.5 by bits 24..26). So in normal
//! multiplayer play the default blurs nothing until the game asks.
//!
//! With the flag set, the motion vector pass (`0x141d5c6b0`) draws
//! `motion_vector_static_with_mb` (its second target the blur velocity,
//! role `0xb0e5c`, R8G8B8A8; `crate::motion`) and then
//! `mblur_mv_depth_composite_dynamic` over the moving models (stencil; this
//! renderer has static motion vectors only, so it is not drawn). Then here
//! (`0x141d64f30`): `mblur_tile_downsample_0` (20 taps across, into role
//! `0xb0e5e`, a twentieth of the width), `_1` (20 down, `0xb0e60`), each
//! with `postFxControl0` = the half-texel shift from the tile's centre to
//! its first pixel and the one-pixel step; `mblur_tile_neighbor`
//! (`0xb0e62`); then `mblur_low` / `_medium` / `_high` by the quality dvar
//! (`0x2853ac00`, Low / Medium / High, default Medium) reading the scene
//! (code texture 7), the velocity (0) and the neighbour tiles (1), into a
//! new scene target that the rest of the chain reads.
//!
//! bo2mp: `BO2MP_T7_MOTION_BLUR` = `off` / `auto` (default, the exe's) /
//! `on`; `BO2MP_T7_MOTION_BLUR_AMOUNT` the Auto amount (default 0, as the
//! exe's refdef starts); `BO2MP_T7_MOTION_BLUR_QUALITY` = 0 / 1 / 2.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;

use render_h1::gpu::Gpu;

use crate::gpu::Tex;
use crate::post_pass::{CodeBuffers, CodePass, CodeTech};

/// The blur dvar's modes (exe enum `143141ff0`).
pub const OFF: i32 = 0;
pub const AUTO: i32 = 1;
pub const ON: i32 = 2;

/// The tile size the downsamples step over (exe `142f87b6c`: 20).
pub const TILE: u32 = 20;

const QUALITY: [&str; 3] = ["mblur_low", "mblur_medium", "mblur_high"];
const TILE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The mode asked for (`BO2MP_T7_MOTION_BLUR`; default the exe's Auto).
pub fn mode() -> i32 {
    match std::env::var("BO2MP_T7_MOTION_BLUR").ok().as_deref().map(str::trim) {
        Some("off") | Some("0") => OFF,
        Some("on") | Some("2") => ON,
        Some("auto") | Some("1") => AUTO,
        _ => crate::quality::get().motion_blur,
    }
}

/// The quality (`BO2MP_T7_MOTION_BLUR_QUALITY`; default the exe's Medium).
pub fn quality() -> usize {
    std::env::var("BO2MP_T7_MOTION_BLUR_QUALITY").ok().and_then(|v| v.trim().parse::<usize>().ok()).filter(|&q| q < 3).unwrap_or(crate::quality::get().motion_blur_quality)
}

/// Auto's blur amount (the refdef's, `BO2MP_T7_MOTION_BLUR_AMOUNT`;
/// default 0).
pub fn amount() -> f32 {
    std::env::var("BO2MP_T7_MOTION_BLUR_AMOUNT").ok().and_then(|v| v.trim().parse::<f32>().ok()).map_or(0.0, |a| a.clamp(0.0, 1.0))
}

/// The view's blur scale as `0x141ddc6c0` makes it (0: no blur): `scale`
/// the scale dvar (1), `amount` the refdef's blur amount (Auto only).
pub fn view_scale(mode: i32, scale: f32, amount: f32) -> f32 {
    match mode {
        ON => scale,
        AUTO => {
            let s = scale * amount;
            if f64::from(s) > 1e-6 { s } else { 0.0 }
        }
        _ => 0.0,
    }
}

/// The tile targets' sizes for a `w` x `h` frame: (downsampled across,
/// then down; the neighbour pass keeps the second).
pub fn tile_sizes(w: u32, h: u32) -> [(u32, u32); 2] {
    let tw = w.div_ceil(TILE);
    [(tw, h), (tw, h.div_ceil(TILE))]
}

/// `postFxControl0` of the two downsamples (`0x141d64f30`): the shift
/// from a tile's centre to its first pixel's, then the one-pixel step.
pub fn tile_controls(w: u32, h: u32) -> [[f32; 4]; 2] {
    let [_, (tw, th)] = tile_sizes(w, h);
    let (fw, fh) = (w as f32, h as f32);
    [[0.5 / fw - 0.5 / tw as f32, 0.0, 1.0 / fw, 0.0], [0.0, 0.5 / fh - 0.5 / th as f32, 0.0, 1.0 / fh]]
}

fn per_scene(w: u32, h: u32) -> Vec<u8> {
    let mut r = [[0f32; 4]; 46];
    r[..25].copy_from_slice(&crate::post_pass::per_scene(w, h));
    let (fw, fh) = (w as f32, h as f32);
    r[44] = [fw, fh, 1.0 / fw, 1.0 / fh];
    r[45] = r[44];
    bytemuck::cast_slice(&r).to_vec()
}

struct Sized {
    width: u32,
    height: u32,
    tiles: [Tex; 3],
    out: Tex,
    /// Per pass: bind groups, vertices.
    groups: Vec<[wgpu::BindGroup; 3]>,
    vertices: Vec<wgpu::Buffer>,
    _consts: Vec<wgpu::Buffer>,
    key: (wgpu::TextureView, wgpu::TextureView),
}

pub struct MotionBlur {
    gpu: Gpu,
    /// tile_downsample_0, _1, tile_neighbor, the blur.
    passes: Vec<CodePass>,
    samplers: HashMap<u16, wgpu::Sampler>,
    buffers: CodeBuffers,
    scene_format: wgpu::TextureFormat,
    pub quality: usize,
    sized: Option<Sized>,
}

impl MotionBlur {
    /// `techs`: core_patch's code techniques (the four passes);
    /// `scene_format`: the lit scene's.
    pub fn new(gpu: &Gpu, techs: &HashMap<String, CodeTech>, scene_format: wgpu::TextureFormat, quality: usize) -> Result<Self, String> {
        let names = ["mblur_tile_downsample_0", "mblur_tile_downsample_1", "mblur_tile_neighbor", QUALITY[quality.min(2)]];
        let mut passes = Vec::new();
        for (k, n) in names.iter().enumerate() {
            let t = techs.get(*n).ok_or_else(|| format!("{n} not in core_patch"))?;
            passes.push(CodePass::new(gpu, t, &[if k == 3 { scene_format } else { TILE_FORMAT }], &[])?);
        }
        let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let mut samplers = HashMap::new();
        for p in &passes {
            for s in p.sampler_states() {
                if let std::collections::hash_map::Entry::Vacant(e) = samplers.entry(s) {
                    e.insert(gpu.device.create_sampler(&crate::sampler::descriptor(s, crate::sampler::ANISO_QUALITY, border)?));
                }
            }
        }
        Ok(Self { gpu: gpu.clone(), passes, samplers, buffers: CodeBuffers::new(gpu), scene_format, quality, sized: None })
    }

    /// The names of the passes this blur draws.
    pub fn pass_names() -> [&'static str; 6] {
        ["mblur_tile_downsample_0", "mblur_tile_downsample_1", "mblur_tile_neighbor", QUALITY[0], QUALITY[1], QUALITY[2]]
    }

    fn make_sized(&self, w: u32, h: u32, scene: &wgpu::TextureView, velocity: &wgpu::TextureView) -> Result<Sized, String> {
        use wgpu::util::DeviceExt;
        let gpu = &self.gpu;
        let [(tw, ht), (tw2, th)] = tile_sizes(w, h);
        let tiles = [
            crate::gpu::target(gpu, "t7 mblur tiles x (role 0xb0e5e)", tw, ht, TILE_FORMAT, wgpu::TextureUsages::COPY_SRC),
            crate::gpu::target(gpu, "t7 mblur tiles (role 0xb0e60)", tw2, th, TILE_FORMAT, wgpu::TextureUsages::COPY_SRC),
            crate::gpu::target(gpu, "t7 mblur neighbour tiles (role 0xb0e62)", tw2, th, TILE_FORMAT, wgpu::TextureUsages::COPY_SRC),
        ];
        let out = crate::gpu::target(gpu, "t7 mblur scene", w, h, self.scene_format, wgpu::TextureUsages::COPY_SRC);
        let dev = &gpu.device;
        let controls = tile_controls(w, h);
        let mut consts = Vec::new();
        let mut groups = Vec::new();
        let mut vertices = Vec::new();
        let sizes = [(tw, ht), (tw2, th), (tw2, th), (w, h)];
        for (k, p) in self.passes.iter().enumerate() {
            let (pw, ph) = sizes[k];
            let ps = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 mblur PerSceneConsts"), contents: &per_scene(pw, ph), usage: wgpu::BufferUsages::UNIFORM });
            let mut c = crate::post::PostFxConsts::default();
            if k < 2 {
                c.control(0, controls[k]);
            }
            let fx = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 mblur PostFxCBuffer"), contents: &c.bytes(), usage: wgpu::BufferUsages::UNIFORM });
            let tex = |id: u32| -> Option<&wgpu::TextureView> {
                match (k, id) {
                    (0, 0x1a) => Some(velocity),
                    (1, 0x1a) => Some(&tiles[0].view),
                    (2, 0x1a) => Some(&tiles[1].view),
                    (3, 0x21) => Some(scene),
                    (3, 0x1a) => Some(velocity),
                    (3, 0x1b) => Some(&tiles[2].view),
                    _ => None,
                }
            };
            groups.push(p.bind(gpu, &ps, &fx, &self.buffers, tex, |s| self.samplers.get(&s))?);
            vertices.push(crate::gpu::buffer(gpu, "t7 mblur triangle", &crate::post_pass::triangle(pw, ph), wgpu::BufferUsages::VERTEX));
            consts.push(ps);
            consts.push(fx);
        }
        Ok(Sized { width: w, height: h, tiles, out, groups, vertices, _consts: consts, key: (scene.clone(), velocity.clone()) })
    }

    /// Records the blur of `scene` by `velocity` (the motion stage's blur
    /// velocity); returns the blurred scene.
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, scene: &wgpu::TextureView, velocity: &wgpu::TextureView, w: u32, h: u32) -> Result<&wgpu::TextureView, String> {
        if self.sized.as_ref().is_none_or(|s| s.width != w || s.height != h || s.key.0 != *scene || s.key.1 != *velocity) {
            self.sized = Some(self.make_sized(w, h, scene, velocity)?);
        }
        let s = self.sized.as_ref().unwrap();
        let targets = [&s.tiles[0].view, &s.tiles[1].view, &s.tiles[2].view, &s.out.view];
        for (k, p) in self.passes.iter().enumerate() {
            p.draw(enc, &[targets[k]], &s.groups[k], &s.vertices[k]);
        }
        Ok(&s.out.view)
    }

    /// The targets of the last run (offline checks): the three tile
    /// targets and the blurred scene.
    pub fn targets(&self) -> Option<[&wgpu::Texture; 4]> {
        let s = self.sized.as_ref()?;
        Some([&s.tiles[0].texture, &s.tiles[1].texture, &s.tiles[2].texture, &s.out.texture])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_scale_follows_the_exe() {
        assert_eq!(view_scale(OFF, 1.0, 1.0), 0.0);
        assert_eq!(view_scale(ON, 1.0, 0.0), 1.0);
        assert_eq!(view_scale(AUTO, 1.0, 0.0), 0.0);
        assert_eq!(view_scale(AUTO, 1.0, 0.5), 0.5);
        assert_eq!(tile_sizes(1920, 1080), [(96, 1080), (96, 54)]);
        let c = tile_controls(1920, 1080);
        // (From tile 0's centre, 0.5/96, to pixel 0's, 0.5/1920.)
        assert!((0.5 / 96.0 + c[0][0] - 0.5 / 1920.0).abs() < 1e-7);
        assert_eq!(c[0][2], 1.0 / 1920.0);
        assert_eq!(c[1][3], 1.0 / 1080.0);
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
            if e == 0 { f / 1024.0 * 2f32.powi(-14) } else { (1.0 + f / 1024.0) * 2f32.powi(e - 15) }
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

    /// The exe's blur passes over a synthetic 160x80 frame: a bright
    /// vertical bar, every pixel moving 10 pixels across. The bar is
    /// smeared sideways (pixels beside it brighten, the bar dims), and with
    /// zero velocity the scene comes back unchanged.
    #[test]
    fn blur_smears_along_the_velocity() {
        let dir = std::path::PathBuf::from(std::env::var("T7_ZONE_DIR").unwrap_or_else(|_| "/mnt/user-data/uploads/zone".into()));
        if !dir.join("core_patch.ff").exists() {
            return eprintln!("skipped: no core_patch.ff");
        }
        let Ok(gpu) = Gpu::headless() else { return eprintln!("skipped: no device") };
        let patch = fastfile_t7::shared::zone(&dir, "core_patch").expect("core_patch");
        let techs = crate::post_pass::code_techs(&patch.image, &patch.capture, &MotionBlur::pass_names()).expect("techs");
        let fmt = wgpu::TextureFormat::Rgba16Float;
        let mut mb = MotionBlur::new(&gpu, &techs, fmt, 1).expect("motion blur");
        let (w, h) = (160u32, 80u32);
        let scene = crate::gpu::target(&gpu, "scene", w, h, fmt, wgpu::TextureUsages::COPY_DST);
        let vel = crate::gpu::target(&gpu, "velocity", w, h, crate::motion::BLUR_VELOCITY_FORMAT, wgpu::TextureUsages::COPY_DST);
        let mut px = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                let v: u16 = if (78..82).contains(&x) { 0x3c00 } else { 0 };
                for c in [v, v, v, 0x3c00] {
                    px.extend_from_slice(&c.to_le_bytes());
                }
            }
        }
        gpu.queue.write_texture_p(scene.texture.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 8), rows_per_image: None }, scene.texture.size());
        let run = |mb: &mut MotionBlur, vx: f32| -> Vec<[f32; 4]> {
            // (The blur velocity encoding: v / 40 + 128/255 per axis, as
            // `_with_mb` writes it after its clamp to 20 pixels.)
            let enc_x = ((vx / 40.0 + 128.0 / 255.0) * 255.0).round() as u8;
            let v: Vec<u8> = (0..w * h).flat_map(|_| [enc_x, 128, 255, 255]).collect();
            gpu.queue.write_texture_p(vel.texture.as_image_copy(), &v, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: None }, vel.texture.size());
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            mb.record(&mut enc, &scene.view, &vel.view, w, h).expect("record");
            gpu.queue.submit([enc.finish()]);
            assert!(pollster::block_on(scope.pop()).is_none(), "validation error");
            read_rgba16f(&gpu, mb.targets().unwrap()[3])
        };
        let still = run(&mut mb, 0.0);
        let row = |img: &[[f32; 4]], y: u32| -> Vec<f32> { (0..w).map(|x| img[(y * w + x) as usize][0]).collect() };
        let r0 = row(&still, 40);
        eprintln!("still row 40 x 74..86: {:?}", &r0[74..86]);
        assert!((r0[80] - 1.0).abs() < 0.02 && r0[70] < 0.02 && r0[90] < 0.02, "no velocity: the scene unchanged");
        let moving = run(&mut mb, 20.0);
        let r1 = row(&moving, 40);
        eprintln!("moving row 40 x 66..94: {:?}", &r1[66..94]);
        let beside: f32 = r1[68..76].iter().chain(&r1[84..92]).sum();
        assert!(beside > 0.5, "the bar smeared sideways ({beside})");
        assert!(r1[80] < 0.95, "the bar dimmed ({})", r1[80]);
    }
}
