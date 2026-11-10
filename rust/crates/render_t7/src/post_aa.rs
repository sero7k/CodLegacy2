//! Anti-aliasing as BlackOps3.exe records it after the post chain's
//! `bloom_apply_lut` (the graded image, render target 0x48, and its luma
//! 0x4a) and before `resample_final` (exe `0x141d66110`).
//!
//! The mode is the AA dvar (hashed `0x99442788`, enum `142f28a04..`: 0
//! off, 1 FXAA, 2 SMAA 1x, 3 Filmic SMAA 1x, 4 SMAA T2x, 5 Filmic SMAA T2x;
//! registered default 5), read through `0x141d634d0`, which turns 3..5 into
//! 2 when the frame cannot be temporal (split screen / several views).
//! Modes 2..5 run `0x141d636c0`, the morphological part, all compute:
//!
//! 1. `smaa_pass1_compute_hq` over the frame (8x8 groups): SRV 1 the luma
//!    (role `0xb0e4c`), SRV 2 the colour (`0xb0e54`), UAV 1 the edges
//!    (`0xb0e30`), UAVs 4 / 5 the horizontal / vertical work queues
//!    (append buffers, initial count 0: one 12-byte element per edge pixel,
//!    the pixel and its neighbour's colour packed);
//! 2. `smaa_resolve_morphological_work_queue_compute` (1 group): the queues'
//!    counts into the indirect arguments (UAV 0), each queue padded with
//!    zero elements to a multiple of 64;
//! 3. `smaa_pass2v_compute`, then `smaa_pass2h_compute`, each dispatched
//!    indirectly (offsets 0 and 12): SRV 0 `$smaa_tex` (the area / search
//!    picture of the `$smaa_pass2` material), SRV 4 the edges, UAV 2 the
//!    colour, written in place at the queued pixels only.
//!
//! Then `0x141d63d80`: mode 3 draws `$smaa_pass4` (the filmic filter over
//! the history, by frame parity) and 4 / 5 the temporal resolve, which
//! needs the sub-pixel jitter of every view projection (not built: see
//! [`mode`]).
//!
//! bo2mp: `BO2MP_T7_AA` picks the mode by the exe's numbers (default the
//! exe's 5). This renderer draws without the projection jitter, so the T2x
//! modes take the exe's own non-temporal fallback (2, as `0x141d634d0`
//! does for a view that cannot be temporal), and Filmic 1x (3) runs as 2
//! as well until its history pass is built; FXAA (1) is not built and runs
//! as off. The D3D append counters are each queue's last word
//! (`dxbc_sm5::UavRef::counter`).

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use render_h1::gpu::Gpu;

use crate::compute::{CodeShader, Program, Res};
use crate::gpu::Tex;

/// The exe's registered default (Filmic SMAA T2x).
pub const EXE_DEFAULT: i32 = 5;

/// What runs for an exe AA mode here: 0 nothing, 2 SMAA 1x.
pub fn effective(mode: i32) -> i32 {
    match mode {
        2..=5 => 2,
        _ => 0,
    }
}

/// The AA mode asked for (`BO2MP_T7_AA`, the exe's numbers; `off` = 0).
pub fn mode() -> i32 {
    match std::env::var("BO2MP_T7_AA").ok().as_deref().map(str::trim) {
        Some("off") | Some("0") => 0,
        Some(v) => v.parse().ok().filter(|m| (0..=5).contains(m)).unwrap_or(EXE_DEFAULT),
        None => crate::quality::get().aa,
    }
}

/// Whether AA runs at all (the mode asked for, as it runs here).
pub fn active() -> bool {
    effective(mode()) != 0
}

/// The graded image's format (render target 0x48): [`COLOUR_FORMAT`] while
/// AA runs, else the post chain's HDR format.
pub fn apply_format(hdr_format: wgpu::TextureFormat) -> wgpu::TextureFormat {
    if active() { COLOUR_FORMAT } else { hdr_format }
}

/// The exe's names for the modes.
pub fn mode_name(m: i32) -> &'static str {
    ["off", "FXAA", "SMAA 1x", "Filmic SMAA 1x", "SMAA T2x", "Filmic SMAA T2x"].get(m as usize).copied().unwrap_or("?")
}

/// The colour target's format while AA runs (the passes write it as a
/// storage texture; R11G11B10 is not one).
pub const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

const NAMES: [&str; 4] = ["smaa_pass1_compute_hq", "smaa_resolve_morphological_work_queue_compute", "smaa_pass2v_compute", "smaa_pass2h_compute"];
/// Bytes of one work queue element.
const ELEMENT: u64 = 12;

struct Sized {
    width: u32,
    height: u32,
    edges: Tex,
    queues: [wgpu::Buffer; 2],
    indirect: wgpu::Buffer,
    groups: Vec<Vec<wgpu::BindGroup>>,
    key: (wgpu::TextureView, wgpu::TextureView),
}

pub struct Smaa {
    gpu: Gpu,
    programs: Vec<Program>,
    sampler: wgpu::Sampler,
    smaa_tex: Tex,
    per_scene: wgpu::Buffer,
    sized: Option<Sized>,
    /// Edge pixels queued by the last run (read back by [`Smaa::counts`]).
    pub report: Vec<String>,
}

impl Smaa {
    /// `shaders`: core_patch's code compute shaders; `smaa_tex`: the
    /// `$smaa_tex` picture.
    pub fn new(gpu: &Gpu, shaders: &std::collections::HashMap<String, CodeShader>, smaa_tex: &fastfile_t7::texture::ImageLevels) -> Result<Self, String> {
        let mut programs = Vec::new();
        for n in NAMES {
            let cs = shaders.get(n).ok_or_else(|| format!("{n} not in core_patch"))?;
            // (The edges, exe 0xb0e30: two channels of 0 / 1.)
            let formats: &[(u32, &str)] = if n == NAMES[0] { &[(1, "rgba8unorm")] } else { &[] };
            programs.push(Program::new(gpu, cs, formats)?);
        }
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("t7 smaa bilinearSampler"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let smaa_tex = crate::gpu::upload(gpu, "$smaa_tex", smaa_tex)?;
        let per_scene = crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some("t7 smaa PerSceneConsts"), size: 45 * 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Ok(Self { gpu: gpu.clone(), programs, sampler, smaa_tex, per_scene, sized: None, report: Vec::new() })
    }

    fn make_sized(&self, w: u32, h: u32, colour: &wgpu::TextureView, luma: &wgpu::TextureView) -> Result<Sized, String> {
        let gpu = &self.gpu;
        let edges = crate::gpu::target(gpu, "t7 smaa edges (rt 0xb0e30)", w, h, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::STORAGE_BINDING);
        // One element per pixel at most, the resolve's padding to 64, and
        // the hidden counter word last.
        let bytes = (u64::from(w) * u64::from(h) + 128) * ELEMENT + 4;
        let q = |label| crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some(label), size: bytes, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let queues = [q("t7 smaa morphologicalWorkQueueH"), q("t7 smaa morphologicalWorkQueueV")];
        let indirect = crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some("t7 smaa indirect"), size: 32, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let mut groups = Vec::new();
        for p in &self.programs {
            groups.push(p.bind(gpu, |s| {
                Some(match s.name.as_str() {
                    "PerSceneConsts" => Res::Buffer(&self.per_scene),
                    "lumaTex" => Res::View(luma),
                    "colorTex" => Res::View(colour),
                    "bilinearSampler" => Res::Sampler(&self.sampler),
                    "edgesRWTex" | "edgesTex" => Res::View(&edges.view),
                    "dstRWTex" => Res::View(colour),
                    "smaaTex" => Res::View(&self.smaa_tex.view),
                    "morphologicalWorkQueueRWBufferH" => Res::Buffer(&queues[0]),
                    "morphologicalWorkQueueRWBufferV" => Res::Buffer(&queues[1]),
                    "indirectRWBuffer" => Res::Buffer(&indirect),
                    _ => return None,
                })
            })?);
        }
        let mut regs = [[0f32; 4]; 45];
        regs[44] = [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32];
        gpu.queue.write_buffer_p(&self.per_scene, 0, bytemuck::cast_slice(&regs));
        Ok(Sized { width: w, height: h, edges, queues, indirect, groups, key: (colour.clone(), luma.clone()) })
    }

    /// Records the morphological AA over `colour` (written in place; a
    /// storage texture of [`COLOUR_FORMAT`]) with its `luma`, `w` x `h`.
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, colour: &wgpu::TextureView, luma: &wgpu::TextureView, w: u32, h: u32) -> Result<(), String> {
        if self.sized.as_ref().is_none_or(|s| s.width != w || s.height != h || s.key.0 != *colour || s.key.1 != *luma) {
            self.sized = Some(self.make_sized(w, h, colour, luma)?);
        }
        let s = self.sized.as_ref().unwrap();
        // (The append counters' initial count, 0, as pass 1 binds them.)
        for q in &s.queues {
            enc.clear_buffer(q, q.size() - 4, Some(4));
        }
        let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 smaa 1x"), timestamp_writes: None });
        let set = |pass: &mut wgpu::ComputePass, k: usize| {
            pass.set_pipeline_p(&self.programs[k].pipeline);
            for (i, g) in s.groups[k].iter().enumerate() {
                pass.set_bind_group_p(i as u32, g, &[]);
            }
        };
        set(&mut pass, 0);
        let [x, y, z] = self.programs[0].groups_for([w, h, 1]);
        pass.dispatch_p(x, y, z);
        set(&mut pass, 1);
        pass.dispatch_p(1, 1, 1);
        set(&mut pass, 2);
        pass.dispatch_workgroups_indirect(&s.indirect, 0);
        set(&mut pass, 3);
        pass.dispatch_workgroups_indirect(&s.indirect, 12);
        Ok(())
    }

    /// The queues of the last run (offline checks): (horizontal, vertical,
    /// indirect).
    pub fn buffers(&self) -> Option<(&wgpu::Buffer, &wgpu::Buffer, &wgpu::Buffer)> {
        let s = self.sized.as_ref()?;
        Some((&s.queues[0], &s.queues[1], &s.indirect))
    }

    /// The edges target of the last run (offline checks).
    pub fn edges(&self) -> Option<&wgpu::Texture> {
        Some(&self.sized.as_ref()?.edges.texture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_follow_the_exe_enum_and_fallback() {
        assert_eq!(mode_name(EXE_DEFAULT), "Filmic SMAA T2x");
        assert_eq!(effective(5), 2);
        assert_eq!(effective(4), 2);
        assert_eq!(effective(2), 2);
        assert_eq!(effective(1), 0);
        assert_eq!(effective(0), 0);
    }

    fn f16(v: f32) -> u16 {
        // (Normal values only: what the test writes.)
        if v == 0.0 {
            return 0;
        }
        let b = v.to_bits();
        let e = ((b >> 23) & 0xff) as i32 - 127 + 15;
        ((b >> 16) & 0x8000) as u16 | ((e as u16) << 10) | ((b >> 13) & 0x3ff) as u16
    }

    fn f32_of(h: u16) -> f32 {
        let e = ((h >> 10) & 0x1f) as i32;
        let m = f32::from(h & 0x3ff);
        let v = if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
        if h & 0x8000 != 0 { -v } else { v }
    }

    fn zone_dir() -> Option<std::path::PathBuf> {
        let d = std::path::PathBuf::from(std::env::var("T7_ZONE_DIR").unwrap_or_else(|_| "/mnt/user-data/uploads/zone".into()));
        d.join("core_patch.ff").exists().then_some(d)
    }

    fn read_texture(gpu: &Gpu, t: &wgpu::Texture, bpp: u32) -> Vec<u8> {
        let (w, h) = (t.width(), t.height());
        let row = (w * bpp).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, t.size());
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let m = buf.slice(..).get_mapped_range();
        let mut out = Vec::new();
        for y in 0..h {
            out.extend_from_slice(&m[(y * row) as usize..(y * row + w * bpp) as usize]);
        }
        out
    }

    fn read_buffer(gpu: &Gpu, b: &wgpu::Buffer, offset: u64, size: u64) -> Vec<u32> {
        let out = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(b, offset, &out, 0, size);
        gpu.queue.submit([enc.finish()]);
        out.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        bytemuck::cast_slice(&out.slice(..).get_mapped_range()).to_vec()
    }

    /// The exe's SMAA 1x over a synthetic 64x64 frame: a dark half-plane
    /// with a slanted edge. The queues hold the edge pixels, the indirect
    /// arguments their 64-groups, and only pixels by the edge change (the
    /// flat areas keep their colour exactly).
    #[test]
    fn smaa_1x_blends_the_edge_only() {
        let Some(dir) = zone_dir() else { return eprintln!("skipped: no core_patch.ff") };
        let Ok(gpu) = Gpu::headless() else { return eprintln!("skipped: no device") };
        let patch = fastfile_t7::shared::zone(&dir, "core_patch").expect("core_patch");
        let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
        let rec = patch.capture.images.iter().find(|i| i.name == "$smaa_tex").expect("$smaa_tex in core_patch");
        let levels = fastfile_t7::texture::zone_levels(rec, &patch.image).expect("$smaa_tex pixels in the zone");
        let mut smaa = Smaa::new(&gpu, &shaders, &levels).expect("smaa");
        let (w, h) = (64u32, 64u32);
        let colour = crate::gpu::target(&gpu, "colour", w, h, COLOUR_FORMAT, wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
        let luma = crate::gpu::target(&gpu, "luma", w, h, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::COPY_DST);
        // Bright above the line y = 20 + x/3, dark below.
        let inside = |x: u32, y: u32| (y as f32) > 20.0 + x as f32 / 3.0;
        let mut px = Vec::new();
        let mut l = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v: f32 = if inside(x, y) { 0.05 } else { 0.9 };
                for c in [v, v, v, 1.0] {
                    px.extend_from_slice(&f16(c).to_le_bytes());
                }
                l.push((v.sqrt() * 255.0) as u8);
            }
        }
        let write = |t: &wgpu::Texture, data: &[u8], bpp: u32| gpu.queue.write_texture_p(t.as_image_copy(), data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * bpp), rows_per_image: None }, t.size());
        write(&colour.texture, &px, 8);
        write(&luma.texture, &l, 1);
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        smaa.record(&mut enc, &colour.view, &luma.view, w, h).expect("record");
        gpu.queue.submit([enc.finish()]);
        assert!(pollster::block_on(scope.pop()).is_none(), "validation error");
        let (qh, qv, ind) = smaa.buffers().unwrap();
        let counts = [read_buffer(&gpu, qh, qh.size() - 4, 4)[0], read_buffer(&gpu, qv, qv.size() - 4, 4)[0]];
        let args = read_buffer(&gpu, ind, 0, 24);
        eprintln!("queues {counts:?} indirect {args:?}");
        // (The resolve's alloc adds one to each count.)
        assert!(counts[0] > 1 && counts[1] > 1, "edge pixels queued: {counts:?}");
        assert_eq!(args[0], (counts[1] - 1).div_ceil(64));
        assert_eq!(args[3], (counts[0] - 1).div_ceil(64));
        let out = read_texture(&gpu, &colour.texture, 8);
        let at = |x: u32, y: u32| f32_of(u16::from_le_bytes([out[((y * w + x) * 8) as usize], out[((y * w + x) * 8 + 1) as usize]]));
        let mut changed = 0;
        let mut far_changed = 0;
        for y in 0..h {
            for x in 0..w {
                let orig = if inside(x, y) { 0.05 } else { 0.9 };
                let v = at(x, y);
                if (v - f32_of(f16(orig))).abs() > 1e-3 {
                    changed += 1;
                    let d = (y as f32 - (20.0 + x as f32 / 3.0)).abs();
                    if d > 3.0 {
                        far_changed += 1;
                    }
                }
            }
        }
        eprintln!("{changed} pixels blended, {far_changed} of them away from the edge");
        assert!(changed > 20, "the edge is blended ({changed})");
        assert_eq!(far_changed, 0, "flat areas untouched");
    }
}
