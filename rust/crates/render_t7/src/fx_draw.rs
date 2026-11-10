//! The effects' sprites drawn as the exe's view list 18 sits in the frame
//! (lists 14, 7, 8, then the effects, then 9, 10, ...): each
//! sprite one instance of its material's forward technique (slot 6, the
//! `ei/effect_*` technique sets) in layout group 8 (one 64-byte record per
//! instance, `crate::fx`), four corners from the vertex id, back to front,
//! into the lit scene over the scene depth.
//!
//! The sprite light map (`lmapTex`, written in the exe by
//! `fx_compute_sprite_light` from the map's lights and probes per sprite)
//! is not built: it is bound as one layer of 1.0, so lit sprites take their
//! colour as authored (see docs/T7_VISUAL_PARITY.md). On by default
//! (`BO2MP_T7_NATIVE_FX=0` turns it off; checked by `crate::fx_check`). The
//! map lane still reads the placed
//! effects for the converted effect runtime, which plays them only while
//! this does not (`frame::settings::t7_native_fx`, set by the native draw
//! from [`Forward::fx_ready`]): a native scene that fails to load, or no
//! sprite material made, falls back to them.
//!
//! Checked offline (`t7_fx_check`, a software adapter, 320x180, one view
//! per effect over a grey scene): the sprites land where the CPU puts them
//! and take the colours their effects are named for.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;
use std::sync::Arc;

use super::{Forward, Targets, Lighting, GBuffer, floatz_slots};
use crate::fx::{SPRITE_RECORD, MAX_SPRITES, Sprite};
use crate::gbuffer::Pipeline;
use crate::layout;

/// Whether render_t7 draws the map's placed effects: on unless
/// `BO2MP_T7_NATIVE_FX=0` (2026-10-27: on by default again, after the
/// particle rules were read off the exe and every staged map's effects
/// pass `fx_check`'s simulation and Nuk3town's spawn-view overlay check).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_T7_NATIVE_FX").is_ok_and(|v| v.trim() == "0"))
}

fn fx_view_cull_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("T7_FX_NO_VIEW_CULL").is_ok_and(|v| v.trim() != "0"))
}

/// Whether a sprite's quad can touch the view: a sphere round its record's
/// centre (world) holding the quad whatever its axes and rotation (the
/// half sizes 0x30 / 0x34 moved by the pivot 0x28 / 0x2c, doubled, plus a
/// margin), against the view's side planes.
fn sprite_in_view(planes: &[[f32; 4]; 4], s: &crate::fx::Sprite) -> bool {
    let f = |o: usize| f32::from_le_bytes(s.record[o..o + 4].try_into().unwrap());
    let (s0, s1) = (f(0x30).abs(), f(0x34).abs());
    let pivot = f(0x28).abs() + f(0x2c).abs();
    let r = (s0 + s1) * (1.0 + pivot) * 2.0 + 16.0;
    if !r.is_finite() {
        return true;
    }
    crate::cull::visible(planes, [f(0), f(4), f(8), r])
}

/// The layout group the effect techniques' declaration is read in.
pub const FX_GROUP: u8 = 8;

pub(super) struct Batch {
    pipeline: Arc<Pipeline>,
    groups: [wgpu::BindGroup; 3],
    reads_depth: bool,
    stencil_ref: u8,
}

#[derive(Default)]
pub struct FxDraw {
    /// Per scene material: its pipeline and bindings (`None`: failed).
    pub(super) batches: HashMap<usize, Option<Batch>>,
    pub(super) records: Option<wgpu::Buffer>,
    pub(super) ids: Option<wgpu::Buffer>,
    pub(super) quad: Option<wgpu::Buffer>,
    pub(super) lmap: Option<crate::gpu::Tex>,
    /// This frame's runs: (material, first sprite, count).
    runs: Vec<(usize, u32, u32)>,
    sprites: Vec<Sprite>,
    /// The sprite records' bytes (kept to be refilled each frame).
    bytes: Vec<u8>,
    pub lines: Vec<String>,
    frames: u64,
    /// The sprite lighting (`fx_compute_sprite_light_shadow`), when on.
    light: Option<Light>,
    /// This view's code constants the sprite lighting reads (its own
    /// `PerSceneConsts`), and the view and inverse view rows.
    light_consts: Vec<u8>,
    view_rows: [[f32; 4]; 4],
    inv_view_rows: [[f32; 4]; 4],
    eye_offset: [f32; 3],
    /// Sprites lit this frame (the nearest; layers 0..n of `lmapTex`).
    lit: u32,
    /// The simulation on its worker, a frame ahead (`BO2MP_T7_FX_INLINE=1`:
    /// on the render thread as before).
    worker: Option<SimWorker>,
    /// The sort's scratch for the inline simulation.
    scratch: crate::fx::SortScratch,
    /// The simulation panicked (on the worker or here): no sprites for the
    /// rest of the map, said once, instead of the game ending.
    sim_failed: bool,
}

/// The effects simulated for `now_ms` from `eye` into `out`; `false` when
/// the simulation panicked (`out` left empty; the panic's message and
/// place are in the run log from the panic hook).
fn simulate(scene: &crate::scene::Scene, now_ms: i32, eye: [f32; 3], out: &mut Vec<Sprite>, scratch: &mut crate::fx::SortScratch) -> bool {
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.fx.sprites_into(now_ms, eye, out, scratch))).is_ok();
    if !ok {
        out.clear();
        *scratch = crate::fx::SortScratch::default();
    }
    ok
}

/// `BO2MP_T7_FX_INLINE=1`: the effects simulated on the render thread at
/// the frame's own time and eye (the old way).
pub fn sim_inline() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_FX_INLINE").is_ok_and(|v| v.trim() == "1"))
}

/// One simulation asked of the worker: the time and eye, and the buffer to
/// fill (handed back and forth, two in all, so nothing is allocated once
/// they have grown).
struct SimJob {
    now_ms: i32,
    eye: [f32; 3],
    out: Vec<Sprite>,
    /// The worker's simulation of it panicked.
    failed: bool,
}

/// The effects' simulation on a thread of its own, one frame ahead: each
/// frame takes the sprites the worker made for it (at the time predicted
/// from the last frame's step, from the last frame's eye) and asks for the
/// next frame's. With a fixed clock and eye the sprites are the ones the
/// render thread would make.
struct SimWorker {
    jobs: std::sync::mpsc::SyncSender<SimJob>,
    /// (In a mutex: the host is shared with the render world, which needs
    /// it `Sync`; only the render thread takes it.)
    done: std::sync::Mutex<std::sync::mpsc::Receiver<SimJob>>,
    /// The job in flight (its time and eye).
    pending: Option<(i32, [f32; 3])>,
    last_now: Option<i32>,
    /// Frames taken from the worker and simulated inline (the report).
    taken: u64,
    inline: u64,
}

impl SimWorker {
    fn spawn(scene: Arc<crate::scene::Scene>) -> Option<Self> {
        let (jobs, rx) = std::sync::mpsc::sync_channel::<SimJob>(1);
        let (tx, done) = std::sync::mpsc::sync_channel::<SimJob>(1);
        std::thread::Builder::new()
            .name("t7 fx sim".into())
            .spawn(move || {
                let mut scratch = crate::fx::SortScratch::default();
                while let Ok(mut job) = rx.recv() {
                    job.failed = !simulate(&scene, job.now_ms, job.eye, &mut job.out, &mut scratch);
                    if tx.send(job).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Self { jobs, done: std::sync::Mutex::new(done), pending: None, last_now: None, taken: 0, inline: 0 })
    }

    /// The sprites for `now_ms` from `eye`: the worker's, when it made them
    /// for close enough a view (a time within `MAX_SKEW_MS`, the frame
    /// clock's step off the prediction; an eye within `MAX_EYE_STEP`),
    /// else made here. Then the next frame's are asked for, into `spare`.
    /// `None`: the simulation panicked (on the worker or here).
    fn frame(&mut self, scene: &crate::scene::Scene, now_ms: i32, eye: [f32; 3], spare: Vec<Sprite>, scratch: &mut crate::fx::SortScratch) -> Option<Vec<Sprite>> {
        const MAX_EYE_STEP: f32 = 256.0;
        const MAX_SKEW_MS: i32 = 50;
        let mut spare = spare;
        let mut out = None;
        if let Some((t, e)) = self.pending.take()
            && let Ok(job) = self.done.get_mut().map_err(|_| ()).and_then(|r| r.recv().map_err(|_| ()))
        {
            if job.failed {
                return None;
            }
            let step = (0..3).map(|k| (e[k] - eye[k]).powi(2)).sum::<f32>().sqrt();
            if (t - now_ms).abs() <= MAX_SKEW_MS && step <= MAX_EYE_STEP {
                out = Some(job.out);
                self.taken += 1;
            } else {
                // (A stall or a camera cut: this frame's own simulation.)
                spare = job.out;
            }
        }
        let out = match out {
            Some(v) => v,
            None => {
                self.inline += 1;
                let mut v = std::mem::take(&mut spare);
                if !simulate(scene, now_ms, eye, &mut v, scratch) {
                    return None;
                }
                v
            }
        };
        let step = self.last_now.map_or(0, |l| (now_ms - l).clamp(0, 100));
        self.last_now = Some(now_ms);
        let next = now_ms.saturating_add(step);
        if self.jobs.send(SimJob { now_ms: next, eye, out: spare, failed: false }).is_ok() {
            self.pending = Some((next, eye));
        }
        Some(out)
    }
}

/// Whether the effect sprites are lit by the exe's sprite light compute
/// shader (`BO2MP_T7_FX_LIGHT=1`; else `lmapTex` is 1.0).
pub fn light_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_FX_LIGHT").is_ok_and(|v| v.trim() == "1"))
}

/// The sprite lighting: `fx_compute_sprite_light_shadow` (core_patch), one
/// thread group of 8x8 per sprite (group z = the sprite, up to
/// `computeSpriteControl.x`), each texel the light at a point of the
/// sprite's quad (the 8x8 grid from -0.5 to 0.5 of its two edges), written
/// to layer z of `lmapTex` (8x8 Rgba16Float), which the effect pixel
/// program samples by the quad's UV at the layer in the record's 0x1c high
/// half. It reads `spriteExtra` (48 bytes: the world centre, the quad's
/// two full edges in view space as halves) and `spriteData` (the records,
/// only repacked into `spriteDataW`, which is not used here).
pub(super) struct Light {
    program: crate::compute::Program,
    groups: Vec<wgpu::BindGroup>,
    extra: wgpu::Buffer,
    per_scene: wgpu::Buffer,
    /// Layers lit per frame at most; layer `layers` stays 1.0 (the rest).
    layers: u32,
}

/// The light map's size per sprite.
const LMAP: u32 = 8;
/// `computeSpriteControl` (code constant 0xcd) in the sprite light
/// program's `PerSceneConsts`.
const SPRITE_CONTROL_AT: usize = 1312;

impl Forward {
    /// Pipelines and bindings for every effect sprite material (once per
    /// target size, with the forward draws).
    pub(super) fn prepare_fx(&mut self, g: &mut GBuffer, targets: &Targets, lighting: &Lighting) {
        let scene = self.scene.clone();
        let mut mats: Vec<usize> = scene.fx.defs.iter().flat_map(|d| &d.elems).flat_map(|e| e.materials.iter().flatten().copied()).collect();
        mats.sort_unstable();
        mats.dedup();
        if mats.is_empty() || !enabled() {
            self.fx.lines.push(format!("t7 fx: render_t7 effect draws {} ({} sprite materials)", if enabled() { "on (BO2MP_T7_NATIVE_FX=0 turns it off)" } else { "off (BO2MP_T7_NATIVE_FX=0)" }, mats.len()));
            return;
        }
        let dev = self.gpu.device.clone();
        if self.fx.records.is_none() {
            self.fx.records = Some(crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 fx sprite records"), size: (MAX_SPRITES * SPRITE_RECORD) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
            self.fx.ids = Some(crate::gpu::buffer(&self.gpu, "t7 fx instance ids", &vec![0u8; MAX_SPRITES * 4], wgpu::BufferUsages::VERTEX));
            // (Corners 0, 1, 2, 3 go round the quad: `t7_fx_techs`'s program.)
            self.fx.quad = Some(crate::gpu::buffer(&self.gpu, "t7 fx quad", bytemuck::cast_slice(&[0u16, 1, 2, 0, 2, 3, 0, 0]), wgpu::BufferUsages::INDEX));
            let texture = crate::audit::texture(&self.gpu, &wgpu::TextureDescriptor {
                label: Some("t7 fx lmapTex (1.0: not lit yet)"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let one = 0x3c00u16.to_le_bytes();
            let texel: Vec<u8> = (0..4).flat_map(|_| one).collect();
            self.gpu.queue.write_texture_p(texture.as_image_copy(), &texel, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: None }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
            self.fx.lmap = Some(crate::gpu::Tex { texture, view });
        }
        if light_enabled() {
            match self.make_fx_light(targets, lighting) {
                Ok(l) => {
                    self.fx.lines.push(format!("t7 fx: lighting: fx_compute_sprite_light_shadow made; up to {} sprites lit a frame (8x8 lmapTex layers), the rest 1.0", l.layers));
                    self.fx.light = Some(l);
                }
                Err(e) => {
                    self.fx.light = None;
                    self.fx.lines.push(format!("t7 fx: lighting: not made: {e} (lmapTex stays 1.0)"));
                }
            }
        } else {
            self.fx.lines.push("t7 fx: lighting: off (BO2MP_T7_FX_LIGHT=1 runs fx_compute_sprite_light_shadow; lmapTex is 1.0)".into());
        }
        self.fx.batches.clear();
        let mut failed = Vec::new();
        for m in mats {
            let Some(t) = scene.materials[m].forward.clone() else { continue };
            let b = match self.pipeline(&t, FX_GROUP) {
                None => {
                    failed.push(format!("{} ({}): pipeline failed", scene.materials[m].name, t.techset));
                    None
                }
                Some(p) => match self.bind(g, &p, &t, m, targets, lighting) {
                    Ok(groups) => Some(Batch { reads_depth: !floatz_slots(&t).is_empty(), stencil_ref: t.state.stencil_ref, pipeline: p, groups }),
                    Err(e) => {
                        failed.push(format!("{} ({}): {e}", scene.materials[m].name, t.techset));
                        None
                    }
                },
            };
            self.fx.batches.insert(m, b);
        }
        let ok = self.fx.batches.values().flatten().count();
        self.fx.lines.push(format!("t7 fx: {ok} of {} sprite materials ready to draw{}", self.fx.batches.len(), if failed.is_empty() { String::new() } else { format!("; not: {}", failed.join("; ")) }));
    }

    /// The sprite lighting's program, buffers and `lmapTex` (which then
    /// replaces the 1x1 stand-in: one 8x8 layer per lit sprite and a last
    /// layer of 1.0).
    fn make_fx_light(&mut self, targets: &Targets, lighting: &Lighting) -> Result<Light, String> {
        let patch = fastfile_t7::shared::zone(&self.scene.zone_dir, "core_patch").ok_or("core_patch.ff not read")?;
        let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
        let cs = shaders.get("fx_compute_sprite_light_shadow").ok_or("fx_compute_sprite_light_shadow not in core_patch")?;
        let program = crate::compute::Program::new(&self.gpu, cs, &[(3, "rgba16float")])?;
        let dev = self.gpu.device.clone();
        let layers = (dev.limits().max_texture_array_layers.saturating_sub(1)).min(2048);
        if layers == 0 {
            return Err("no texture array layers".into());
        }
        let texture = crate::audit::texture(&self.gpu, &wgpu::TextureDescriptor {
            label: Some("t7 fx lmapTex"),
            size: wgpu::Extent3d { width: LMAP, height: LMAP, depth_or_array_layers: layers + 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let one: Vec<u8> = (0..LMAP * LMAP * 4).flat_map(|_| 0x3c00u16.to_le_bytes()).collect();
        self.gpu.queue.write_texture_p(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: layers }, aspect: wgpu::TextureAspect::All },
            &one,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(LMAP * 8), rows_per_image: None },
            wgpu::Extent3d { width: LMAP, height: LMAP, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let extra = crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 fx spriteExtra"), size: u64::from(layers) * 48, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let data_w = crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 fx spriteDataW (not read)"), size: u64::from(layers) * 64, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let scene_size = program.slots.iter().find_map(|s| match (&s.kind, s.name.as_str()) {
            (crate::compute::SlotKind::Uniform { size }, "PerSceneConsts") => Some(*size),
            _ => None,
        });
        let per_scene = crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 fx sprite light PerSceneConsts"), size: scene_size.unwrap_or(1376).max(SPRITE_CONTROL_AT as u64 + 16), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let records = self.fx.records.as_ref().ok_or("no sprite records")?;
        let masks = self.cull.as_ref().ok_or("light culling not made")?;
        let groups = program.bind(&self.gpu, |slot| match slot.name.as_str() {
            "PerSceneConsts" => Some(crate::compute::Res::Buffer(&per_scene)),
            "spriteData" => Some(crate::compute::Res::Buffer(records)),
            "spriteExtra" => Some(crate::compute::Res::Buffer(&extra)),
            "spriteDataW" => Some(crate::compute::Res::Buffer(&data_w)),
            "lmapTex" => Some(crate::compute::Res::View(&view)),
            "visibleLights" => Some(crate::compute::Res::View(&masks.masks[0].view)),
            "visibleProbes" => Some(crate::compute::Res::View(&masks.masks[1].view)),
            n => match &slot.kind {
                crate::compute::SlotKind::Sampler { .. } => lighting.named_sampler(n).map(crate::compute::Res::Sampler),
                _ => lighting.named(n, targets),
            },
        })?;
        self.fx.light_consts = vec![0; per_scene.size() as usize];
        self.fx.lmap = Some(crate::gpu::Tex { texture, view });
        drop(data_w);
        Ok(Light { program, groups, extra, per_scene, layers })
    }

    /// The view's code constants for the sprite lighting (from
    /// [`Forward::update`]).
    pub(super) fn fx_consts(&mut self, consts: &crate::code::CodeConsts) {
        let Some(l) = self.fx.light.as_ref() else { return };
        let mut missing = Vec::new();
        let mut bytes = std::mem::take(&mut self.fx.light_consts);
        crate::lighting::fill_code(&mut bytes, &l.program.args, consts, &mut missing);
        self.fx.light_consts = bytes;
        let rows = |i: u16| -> [[f32; 4]; 4] {
            let mut m = [[0.0; 4]; 4];
            for (k, r) in consts.get(i).unwrap_or(&[]).iter().take(4).enumerate() {
                m[k] = *r;
            }
            m
        };
        self.fx.view_rows = rows(0xd5);
        self.fx.inv_view_rows = rows(0xd6);
        let e = consts.get(0xc3).and_then(|r| r.first()).copied().unwrap_or_default();
        self.fx.eye_offset = [e[0], e[1], e[2]];
    }

    /// The nearest sprites' `spriteExtra` records and light map layers (the
    /// rest take the layer of 1.0); the quad's edges as the effect vertex
    /// program builds them (see `t7_fx_techs`'s dump).
    fn fx_light_sprites(&mut self, sprites: &mut [Sprite]) {
        let Some(l) = self.fx.light.as_ref() else { return };
        let n = sprites.len().min(l.layers as usize);
        let first = sprites.len() - n;
        let mut extra = vec![0u8; n * 48];
        let (inv, view, eye) = (self.fx.inv_view_rows, self.fx.view_rows, glam::Vec3::from(self.fx.eye_offset));
        let row = |r: [f32; 4]| glam::Vec3::new(r[0], r[1], r[2]);
        for (i, s) in sprites.iter_mut().enumerate() {
            let layer = if i >= first { (i - first) as u32 } else { l.layers };
            let w = u32::from_le_bytes(s.record[0x1c..0x20].try_into().unwrap());
            s.record[0x1c..0x20].copy_from_slice(&((w & 0xffff) | layer << 16).to_le_bytes());
            if i < first {
                continue;
            }
            let r = &s.record;
            let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
            let h = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
            let un = |v: u32| glam::Vec3::new((v & 1023) as f32, (v >> 10 & 1023) as f32, (v >> 20 & 1023) as f32) / 1023.0 * 2.00196 - glam::Vec3::splat(1.00196);
            let centre = glam::Vec3::new(f(0), f(4), f(8));
            let flags = h(0x3c);
            let d = (eye - centre).normalize_or_zero();
            let (up, right, fwd) = (row(inv[1]), row(inv[0]), row(inv[2]));
            let side = if flags & 1 != 0 { up.cross(d).normalize_or_zero() } else { right };
            let flat = if flags & 1 != 0 { glam::Vec2::new(d.x, d.y) } else { -glam::Vec2::new(fwd.x, fwd.y) };
            let flat = flat.normalize_or_zero();
            let (mut a, mut b) = if flags & 2 != 0 { (glam::Vec3::Z, glam::Vec3::new(-flat.y, flat.x, 0.0)) } else { (up, side) };
            if flags & 4 != 0 {
                a = un(u32::from_le_bytes(r[0x20..0x24].try_into().unwrap()));
                b = un(u32::from_le_bytes(r[0x24..0x28].try_into().unwrap()));
            }
            let rot = f32::from(h(0x3e)) * (std::f32::consts::TAU / 65536.0);
            let (sn, cs) = rot.sin_cos();
            let uh = (a * sn + b * cs) * f(0x30);
            let vh = (b * sn - a * cs) * f(0x34);
            let c = centre + uh * f(0x28) + vh * f(0x2c);
            let to_view = |v: glam::Vec3| row(view[0]) * v.x + row(view[1]) * v.y + row(view[2]) * v.z;
            let (x, y) = (to_view(uh * 2.0), to_view(vh * 2.0));
            let hf = |v: f32| u32::from(crate::fx::half(v));
            let words = [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), hf(x.x) | hf(x.y) << 16, hf(x.z) | hf(y.x) << 16, hf(y.y) | hf(y.z) << 16, 0, 0, 0, 0, 0, 0];
            let at = (i - first) * 48;
            extra[at..at + 48].copy_from_slice(bytemuck::cast_slice(&words));
        }
        self.fx.lit = n as u32;
        if n > 0 {
            self.gpu.queue.write_buffer_p(&l.extra, 0, &extra);
        }
        let mut bytes = std::mem::take(&mut self.fx.light_consts);
        if bytes.len() >= SPRITE_CONTROL_AT + 16 {
            bytes[SPRITE_CONTROL_AT..SPRITE_CONTROL_AT + 16].copy_from_slice(bytemuck::cast_slice(&[n as u32, 0, 0, 0]));
            self.gpu.queue.write_buffer_p(&l.per_scene, 0, &bytes);
        }
        self.fx.light_consts = bytes;
    }

    /// This frame's sprites (`now_ms` on the frame clock, the eye in world
    /// units), uploaded.
    pub fn update_fx(&mut self, now_ms: i32, eye: [f32; 3]) {
        self.fx.runs.clear();
        if self.fx.batches.is_empty() || self.fx.sim_failed {
            return;
        }
        let spare = std::mem::take(&mut self.fx.sprites);
        if self.fx.worker.is_none() && !sim_inline() {
            self.fx.worker = SimWorker::spawn(self.scene.clone());
        }
        let sprites = match self.fx.worker.as_mut() {
            Some(w) => w.frame(&self.scene, now_ms, eye, spare, &mut self.fx.scratch),
            None => {
                let mut v = spare;
                simulate(&self.scene, now_ms, eye, &mut v, &mut self.fx.scratch).then_some(v)
            }
        };
        let Some(mut sprites) = sprites else {
            // (The panic's message and place are the lines above it.)
            self.fx.sim_failed = true;
            self.fx.worker = None;
            self.fx.lines.push(format!("t7 fx: the effects simulation panicked at {now_ms} ms (the CRASH panic lines above say where); no effect sprites for the rest of this map, the game goes on"));
            return;
        };
        sprites.retain(|s| self.fx.batches.get(&s.material).is_some_and(Option::is_some));
        // (`T7_FX_SPRITES=<n>`: the nearest n only, to check one sprite's pixels offline.)
        static SPRITES: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
        static DEBUG_SIZE: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
        if let Some(n) = *SPRITES.get_or_init(|| std::env::var("T7_FX_SPRITES").ok().and_then(|v| v.parse::<usize>().ok())) {
            let drop = sprites.len().saturating_sub(n);
            sprites.drain(..drop);
        }
        // (`T7_FX_DEBUG_SIZE=<units>`: every sprite that size and opaque, to
        // check its placement and axes offline.)
        if let Some(size) = *DEBUG_SIZE.get_or_init(|| std::env::var("T7_FX_DEBUG_SIZE").ok().and_then(|v| v.parse::<f32>().ok())) {
            for s in &mut sprites {
                s.record[0x30..0x34].copy_from_slice(&size.to_le_bytes());
                s.record[0x34..0x38].copy_from_slice(&size.to_le_bytes());
                s.record[0x13] = 255;
            }
        }
        self.fx_light_sprites(&mut sprites);
        // Sprites wholly outside the view draw no pixels: left out of the
        // records and the draws (after the lighting picked its sprites, so
        // the lit set is the same; the order of the rest is kept, so runs of
        // one material that only off-screen sprites split become one draw).
        // `T7_FX_NO_VIEW_CULL=1`: every sprite.
        if let Some(p) = self.planes.filter(|_| !fx_view_cull_off()) {
            sprites.retain(|s| sprite_in_view(&p, s));
        }
        crate::timing::count_fx_quads(sprites.len() as u64);
        // (The records' bytes in a buffer kept from frame to frame.)
        let mut bytes = std::mem::take(&mut self.fx.bytes);
        bytes.clear();
        for (i, s) in sprites.iter().enumerate() {
            bytes.extend_from_slice(&s.record);
            match self.fx.runs.last_mut() {
                Some((m, _, n)) if *m == s.material => *n += 1,
                _ => self.fx.runs.push((s.material, i as u32, 1)),
            }
        }
        if let Some(b) = &self.fx.records
            && !bytes.is_empty()
        {
            self.gpu.queue.write_buffer_p(b, 0, &bytes);
        }
        if self.fx.frames == 0 || self.fx.frames.is_power_of_two() && self.fx.frames >= 64 && self.fx.frames <= 4096 {
            let sim = match &self.fx.worker {
                Some(w) => format!("simulated on its worker a frame ahead ({} frames taken from it, {} made here; BO2MP_T7_FX_INLINE=1: on the render thread)", w.taken, w.inline),
                None => "simulated on the render thread".into(),
            };
            self.fx.lines.push(format!("t7 fx: frame {} at {now_ms} ms: {} sprites in {} draws ({} placed effects), {sim}", self.fx.frames, sprites.len(), self.fx.runs.len(), self.scene.fx.placed.len()));
        }
        self.fx.frames += 1;
        self.fx.sprites = sprites;
        self.fx.bytes = bytes;
    }

    /// Records the effect sprites (view list 18).
    pub(super) fn draw_fx(&self, enc: &mut wgpu::CommandEncoder, targets: &Targets, scene: &wgpu::TextureView) {
        if self.fx.runs.is_empty() {
            return;
        }
        let (Some(records), Some(ids), Some(quad)) = (&self.fx.records, &self.fx.ids, &self.fx.quad) else { return };
        // The sprite lighting into lmapTex, after the light culling.
        if let Some(l) = self.fx.light.as_ref()
            && self.fx.lit > 0
        {
            let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 fx sprite light"), timestamp_writes: None });
            pass.set_pipeline_p(&l.program.pipeline);
            for (k, g) in l.groups.iter().enumerate() {
                pass.set_bind_group_p(k as u32, g, &[]);
            }
            pass.dispatch_p(1, 1, self.fx.lit);
        }
        if self.fx.runs.iter().any(|(m, _, _)| self.fx.batches.get(m).and_then(Option::as_ref).is_some_and(|b| b.reads_depth)) {
            self.copy_depth(enc, targets);
        }
        let ops = Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store });
        let mut rp = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("t7 effects"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: scene, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &targets.depth.view, depth_ops: ops, stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }) }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_index_buffer(quad.slice(..), wgpu::IndexFormat::Uint16);
        for &(m, first, n) in &self.fx.runs {
            let Some(Some(b)) = self.fx.batches.get(&m) else { continue };
            rp.set_pipeline_p(&b.pipeline.pipeline);
            rp.set_stencil_reference(u32::from(b.stencil_ref));
            for (gi, bg) in b.groups.iter().enumerate() {
                rp.set_bind_group_p(gi as u32, bg, &[]);
            }
            for (k, &slot) in b.pipeline.slots.iter().enumerate() {
                let buf = if slot == layout::INSTANCE_SLOT { ids } else { records };
                rp.set_vertex_buffer(k as u32, buf.slice(..));
            }
            rp.draw_indexed_p(0..6, 0, first..first + n);
        }
    }

    /// Whether the placed effects are drawn here (on, and at least one
    /// sprite material made).
    pub fn fx_ready(&self) -> bool {
        enabled() && self.fx.batches.values().any(Option::is_some)
    }

    /// The sprite light map and how many layers were lit this frame (checks).
    pub fn fx_lmap(&self) -> Option<(&wgpu::Texture, u32)> {
        self.fx.lmap.as_ref().map(|t| (&t.texture, self.fx.lit))
    }

    /// The `t7 fx:` lines gathered since the last call.
    pub fn take_fx_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.fx.lines)
    }
}
