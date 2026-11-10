//! bo2mp: Black Ops III's out-of-bounds filter (`_oob.csc` enables the
//! filter material `generic_filter_out_of_bounds`, technique set
//! `out_of_bounds#da7372ff` in core_common, program
//! `postfx/2d_out_of_bounds.hlsl`) drawn with the game's own programs.
//!
//! What its pixel program does (read off the DXBC, translated): it reads the
//! graded image the chain's `bloom_apply_lut` leaves in 0x48 (`frameBuffer`,
//! code texture 0xa, ·2^-15 as `resample_final` reads it), warps the read by
//! the static texture (`staticTexture` = `i_generic_filter_out_of_bounds`,
//! scrolled by `gameTime`, `warp`/`speed` over `renderTargetSize`), greys it
//! (`pow(luma + brightness, contrast)` blended in by `opacity`), multiplies
//! it by the static (`staticTile`, `staticAmount`) and the rolling bar of
//! `lookupTexture` (`tv_lookup`, `lookupSpeed`, `lookupMinX`..`lookupMaxX`),
//! and writes it back ·2^15. So it is drawn here between `bloom_apply_lut`
//! and `resample_final`, over a copy of 0x48, into 0x48. The material's own
//! `$Globals` (warp 10, speed 6, staticTile 25, staticAmount 0.9,
//! lookupSpeed 1.3, lookupMaxX 0.8, lookupMinX 0.7, contrast 1, brightness
//! 0.5, opacity 0.07) are bound as the material carries them.
//!
//! UNVERIFIED: where the exe draws the filter list (placed by what the
//! program reads), `gameTime`'s components (all four set to seconds) and
//! `renderTargetSize` (w, h, 1/w, 1/h); the filter's script constants
//! (`set_filter_oob_*`) are not decoded, so the material's values are used
//! whole while the player is out of bounds.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::sync::atomic::{AtomicBool, Ordering};

use render_h1::gpu::Gpu;

use crate::gpu::Tex;
use crate::post_pass::{CodeBuffers, CodePass, CodeTech};

/// The filter's technique set and material (core_common).
pub const TECHSET: &str = "out_of_bounds";
pub const MATERIAL: &str = "generic_filter_out_of_bounds";
/// Code texture `resolvedScene` (the program's `frameBuffer`).
const CODE_FRAME: u32 = 0xa;
/// `PerSceneConsts` registers the program reads: `renderTargetSize` 44,
/// `gameTime` 69.
const REGISTERS: usize = 70;
const REG_TARGET_SIZE: usize = 44;
const REG_GAME_TIME: usize = 69;

/// Is the local player out of bounds (set each frame by the client).
static ON: AtomicBool = AtomicBool::new(false);

pub fn set_active(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn active() -> bool {
    ON.load(Ordering::Relaxed)
}

/// `PerSceneConsts` for the filter's draw into a `w` × `h` target at `t`
/// seconds.
pub fn per_scene(w: u32, h: u32, t: f32) -> Vec<[f32; 4]> {
    let mut r = vec![[0f32; 4]; REGISTERS];
    for (i, v) in crate::post_pass::per_scene(w, h).iter().enumerate() {
        r[i] = *v;
    }
    r[REG_TARGET_SIZE] = [w as f32, h as f32, 1.0 / w.max(1) as f32, 1.0 / h.max(1) as f32];
    r[REG_GAME_TIME] = [t; 4];
    r
}

/// The `$Globals` constants of the material (its first buffer of that name).
pub fn material_globals(cap: &fastfile_t7::walk::capture::Capture) -> Option<Vec<u8>> {
    let m = cap.materials.iter().find(|m| m.name.trim_start_matches(',') == MATERIAL)?;
    m.cbuffers.iter().rev().find(|(_, n, d)| n == "$Globals" && d.len() >= 208).map(|(_, _, d)| d.clone())
}

/// The filter's technique (5) from core_common, with the material's samplers.
pub fn tech(image: &[u8], cap: &fastfile_t7::walk::capture::Capture) -> Result<CodeTech, String> {
    let ts = cap
        .techsets
        .iter()
        .find(|t| t.name.trim_start_matches(',').split('#').next() == Some(TECHSET))
        .ok_or("out_of_bounds: no technique set in core_common")?;
    let ti = ts.techniques[crate::post_pass::CODE_TECHNIQUE].as_ref().ok_or("out_of_bounds: no technique 5")?;
    let prog = |k: usize| -> Result<Vec<u8>, String> {
        let (at, len) = ti.programs[k].ok_or_else(|| format!("out_of_bounds: no program {k}"))?;
        image.get(at..at + len).map(<[u8]>::to_vec).ok_or_else(|| "out_of_bounds: program past the zone".to_owned())
    };
    let state = ti.state_map.and_then(|s| image.get(s..s + 112)).and_then(crate::state::StateMap::read).ok_or("out_of_bounds: no state map")?;
    let samplers = cap
        .materials
        .iter()
        .find(|m| m.name.trim_start_matches(',') == MATERIAL)
        .map(|m| m.sampler_defs.iter().map(|d| (u32::from_le_bytes(d[0..4].try_into().unwrap()), u16::from_le_bytes([d[4], d[5]]) & 0x3fff)).collect())
        .ok_or("generic_filter_out_of_bounds: material not in core_common")?;
    Ok(CodeTech { name: TECHSET.to_owned(), vs: prog(0)?, ps: prog(1)?, args: ti.args.clone(), state, samplers })
}

/// The filter made for the chain.
pub struct OobFilter {
    pass: CodePass,
    globals: wgpu::Buffer,
    per_scene: wgpu::Buffer,
    triangle: Option<((u32, u32), wgpu::Buffer)>,
    /// (name hash, picture) of the material's textures.
    pictures: Vec<(u32, Tex)>,
    samplers: Vec<(u16, wgpu::Sampler)>,
    copy: Option<Tex>,
    group: Option<[wgpu::BindGroup; 3]>,
    group_key: Option<wgpu::TextureView>,
    buffers: CodeBuffers,
    format: wgpu::TextureFormat,
    start: std::time::Instant,
    pub drawn: u64,
}

impl OobFilter {
    /// `format`: the graded image's (0x48) format.
    pub fn new(gpu: &Gpu, zone_dir: &std::path::Path, format: wgpu::TextureFormat) -> Result<Self, String> {
        use wgpu::util::DeviceExt;
        let common = fastfile_t7::shared::zone(zone_dir, "core_common").ok_or("core_common.ff not read")?;
        let t = tech(&common.image, &common.capture)?;
        let pass = CodePass::new(gpu, &t, &[format], &[])?;
        let globals = material_globals(&common.capture).ok_or("generic_filter_out_of_bounds: no $Globals")?;
        let dev = &gpu.device;
        let globals = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 oob $Globals"), contents: &globals, usage: wgpu::BufferUsages::UNIFORM });
        let per_scene = crate::audit::buffer(dev, &wgpu::BufferDescriptor { label: Some("t7 oob PerSceneConsts"), size: (REGISTERS * 16) as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let m = common.capture.materials.iter().find(|m| m.name.trim_start_matches(',') == MATERIAL).ok_or("material gone")?;
        let mut pictures = Vec::new();
        for (hash, name) in &m.textures {
            let name = name.trim_start_matches(',');
            // (Both pictures are core_common's own, in its zone.)
            let rec = common.capture.images.iter().find(|i| i.name.trim_start_matches(',') == name).ok_or_else(|| format!("oob picture {name}: not in core_common"))?;
            let im = fastfile_t7::texture::zone_levels(rec, &common.image).map_err(|e| format!("oob picture {name}: {e:?}"))?;
            pictures.push((*hash, crate::gpu::upload(gpu, name, &im)?));
        }
        let border = dev.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let mut samplers = Vec::new();
        for s in pass.sampler_states() {
            samplers.push((s, dev.create_sampler(&crate::sampler::descriptor(s, crate::sampler::ANISO_QUALITY, border)?)));
        }
        Ok(Self { pass, globals, per_scene, triangle: None, pictures, samplers, copy: None, group: None, group_key: None, buffers: CodeBuffers::new(gpu), format, start: std::time::Instant::now(), drawn: 0 })
    }

    /// Records the filter over `image` (0x48: copied, then drawn into).
    pub fn draw(&mut self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, image: &Tex) -> Result<(), String> {
        use wgpu::util::DeviceExt;
        let size = image.texture.size();
        let (w, h) = (size.width, size.height);
        if self.copy.as_ref().is_none_or(|c| c.texture.size() != size) {
            self.copy = Some(crate::gpu::target(gpu, "t7 oob frame copy", w, h, self.format, wgpu::TextureUsages::COPY_DST));
            self.group = None;
            self.triangle = None;
        }
        if self.group_key.as_ref() != Some(&image.view) {
            self.group = None;
        }
        if self.triangle.as_ref().is_none_or(|t| t.0 != (w, h)) {
            let vb = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 oob triangle"), contents: &crate::post_pass::triangle(w, h), usage: wgpu::BufferUsages::VERTEX });
            self.triangle = Some(((w, h), vb));
        }
        let t = self.start.elapsed().as_secs_f32() % 1000.0;
        gpu.queue.write_buffer_p(&self.per_scene, 0, bytemuck::cast_slice(&per_scene(w, h, t)));
        if self.group.is_none() {
            let copy = &self.copy.as_ref().unwrap().view;
            let pictures = &self.pictures;
            let samplers = &self.samplers;
            let g = self.pass.bind_with(
                gpu,
                &self.per_scene,
                &self.per_scene,
                &[(0, &self.globals)],
                &self.buffers,
                |id| if id == CODE_FRAME { Some(copy) } else { pictures.iter().find(|(hh, _)| *hh == id).map(|(_, p)| &p.view) },
                |s| samplers.iter().find(|(k, _)| *k == s).map(|(_, x)| x),
            )?;
            self.group = Some(g);
            self.group_key = Some(image.view.clone());
        }
        enc.copy_texture_to_texture(image.texture.as_image_copy(), self.copy.as_ref().unwrap().texture.as_image_copy(), size);
        self.pass.draw(enc, &[&image.view], self.group.as_ref().unwrap(), &self.triangle.as_ref().unwrap().1);
        self.drawn += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_scene_carries_target_size_and_time() {
        let r = per_scene(1280, 720, 2.5);
        assert_eq!(r.len(), 70);
        assert_eq!(r[44], [1280.0, 720.0, 1.0 / 1280.0, 1.0 / 720.0]);
        assert_eq!(r[69], [2.5; 4]);
        // The code draw's pixel-to-clip transform stays.
        assert_eq!(r[11], [-1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn switch() {
        set_active(true);
        assert!(active());
        set_active(false);
        assert!(!active());
    }

    /// The filter's programs and material from the staged core_common
    /// (`T7_ZONE_DIR`): technique 5 has both programs, the pixel program
    /// reads `frameBuffer` (0xa) and the two material pictures, and the
    /// material's `$Globals` carry BO3's numbers.
    #[test]
    #[ignore = "needs the staged core_common.ff (T7_ZONE_DIR)"]
    fn staged_core_common_filter() {
        let dir = std::env::var("T7_ZONE_DIR").expect("T7_ZONE_DIR");
        let z = fastfile_t7::shared::zone(std::path::Path::new(&dir), "core_common").expect("core_common");
        let t = tech(&z.image, &z.capture).expect("technique");
        assert!(!t.vs.is_empty() && !t.ps.is_empty());
        assert!(t.args.iter().any(|a| a.ty == 0x0501 && a.u[0] == CODE_FRAME));
        assert_eq!(t.args.iter().filter(|a| a.ty == 0x0201).count(), 2);
        let g = material_globals(&z.capture).expect("$Globals");
        let f = |at: usize| f32::from_le_bytes(g[at..at + 4].try_into().unwrap());
        assert_eq!([f(0xa0), f(0xa4), f(0xa8)], [10.0, 6.0, 25.0]);
        assert!((f(0xc4) - 0.07).abs() < 1e-6);
    }
}
