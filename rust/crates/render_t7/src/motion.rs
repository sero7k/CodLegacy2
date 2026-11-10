//! Static motion vectors as the exe draws them (`0x141d5c6b0`, after the
//! uber resolve, before the AO): `motion_vector_static` (or `_with_mb`
//! with motion blur, not drawn here) over the screen into the frame's
//! motion-vector target (render target 0x16/0x17 by frame parity, view
//! role `0xb0e58`; R16G16_SNORM), reading `floatZ` (code texture 0x10, the
//! scene depth) and `MotionVectorParams` (constant buffer 7), which the
//! view setup fills through `0x141ddc6c0` from the previous and the current
//! view:
//!
//! * registers 0..3: the previous view-projection (`prevView ×
//!   prevProjection`, `0x142388870`) re-based on the previous eye (row 3 =
//!   `prevEye · rows 0..2 + row 3`), so that it takes positions relative
//!   to the previous eye;
//! * 4..7: the same for a view at the previous eye with the rotation
//!   slerped (`t` = 1) to the current one (motion blur; the static program
//!   doesn't read it);
//! * 8..11: the inverse of the current eye-relative view (view space →
//!   eye-relative world);
//! * 12: the previous eye, `w` the motion blur scale (0 here);
//! * 13: current eye − previous eye;
//! * 14: the inverse projection's `[0]` and `[5]` (`tan` half angles).
//!
//! With the temporal anti-aliasing modes (4/5) the exe also multiplies in
//! its sub-pixel jitter (`0x141ddc5a0`); this renderer draws without jitter
//! (`subpixelOffset` 0), so the jitter is the identity here.
//!
//! The exe's depth buffer is not ours: its programs decode `floatZ` as
//! `z = 1 / (64·d − 63)` for `d ≥ 63/64` (the view model's range) and `z =
//! 63 / (64·d)` below it (the world), while our G-buffer depth is `near /
//! z` (reversed, infinite). [`Motion`] therefore writes `floatZ` in the
//! exe's encoding from our depth (world range: `d · 63 / (64·near)`) before
//! the exe's program runs; nothing else is changed.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::gpu::Tex;
use crate::post_pass::{code_techs, per_scene, triangle, CodeBuffers, CodePass};

const REGISTERS: usize = 72;

fn rows(m: glam::Mat4) -> [[f32; 4]; 4] {
    let r = crate::code::rows(m);
    [r[0], r[1], r[2], r[3]]
}

/// `MotionVectorParams` (60 floats) for the current and previous views.
pub fn params(cur: &Camera, prev: &Camera) -> [[f32; 4]; 15] {
    let mut p = [[0f32; 4]; 15];
    // Eye-relative view-projections already take positions relative to
    // their own eye (= the exe's re-based row 3).
    p[0..4].copy_from_slice(&rows(prev.view_projection));
    let prev_proj = prev.view_projection * prev.rotation.inverse();
    p[4..8].copy_from_slice(&rows(prev_proj * cur.rotation));
    p[8..12].copy_from_slice(&rows(cur.rotation.inverse()));
    p[12] = [prev.eye.x, prev.eye.y, prev.eye.z, 0.0];
    let d = cur.eye - prev.eye;
    p[13] = [d.x, d.y, d.z, 0.0];
    let proj = cur.view_projection * cur.rotation.inverse();
    let inv = proj.inverse();
    p[14] = [inv.x_axis.x, inv.y_axis.y, 0.0, 0.0];
    p
}

const CONVERT: &str = r#"
@group(0) @binding(0) var depth: texture_depth_2d;
struct K { k: vec4<f32> }
@group(0) @binding(1) var<uniform> k: K;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(p.xy), 0);
    return vec4<f32>(d * k.k.x, 0.0, 0.0, 0.0);
}
"#;

struct Sized {
    float_z: Tex,
    /// Render targets 0x16 / 0x17 (by frame parity).
    velocity: [Tex; 2],
    convert: wgpu::BindGroup,
    groups: [wgpu::BindGroup; 3],
    vertices: wgpu::Buffer,
    /// bo2mp: with motion blur, the blur's velocity (view role `0xb0e5c`,
    /// R8G8B8A8_UNORM: the screen velocity clamped to 20 pixels, encoded
    /// `v / 40 + 128/255` with v in pixels) and `motion_vector_static_with_mb`'s groups.
    blur: Option<(Tex, [wgpu::BindGroup; 3])>,
}

/// The blur velocity's format (role `0xb0e5c`).
pub const BLUR_VELOCITY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Motion {
    gpu: Gpu,
    pass: CodePass,
    args: Vec<fastfile_t7::walk::capture::ShaderArg>,
    buffers: CodeBuffers,
    per_scene: wgpu::Buffer,
    post_fx: wgpu::Buffer,
    params: wgpu::Buffer,
    convert_k: wgpu::Buffer,
    convert: wgpu::RenderPipeline,
    pub format: wgpu::TextureFormat,
    sized: Option<Sized>,
    prev: Option<Camera>,
    /// bo2mp: `motion_vector_static_with_mb` (motion blur on: the exe's
    /// `0x141d5c6b0` draws it in the static program's place, its second
    /// target the blur velocity).
    with_mb: Option<CodePass>,
    /// The view's motion blur scale (`MotionVectorParams` 12.w; 0: off).
    blur_scale: f32,
}

impl Motion {
    pub fn new(gpu: &Gpu, image: &[u8], cap: &fastfile_t7::walk::capture::Capture) -> Result<Self, String> {
        let techs = code_techs(image, cap, &["motion_vector_static"])?;
        let t = &techs["motion_vector_static"];
        // R16G16_SNORM, as a render target only with 16-bit norm formats.
        let format = if gpu.device.features().contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) { wgpu::TextureFormat::Rg16Snorm } else { wgpu::TextureFormat::Rg16Float };
        let pass = CodePass::new(gpu, t, &[format], &[0x10])?;
        let mb_tech = code_techs(image, cap, &["motion_vector_static_with_mb"]).ok().map(|mut m| m.remove("motion_vector_static_with_mb").unwrap());
        let with_mb = mb_tech.as_ref().and_then(|t| CodePass::new(gpu, t, &[format, BLUR_VELOCITY_FORMAT], &[0x10]).ok());
        let mut args = t.args.clone();
        if let Some(m) = &mb_tech {
            args.extend(m.args.iter().filter(|a| !t.args.iter().any(|b| b.ty == a.ty && b.location == a.location)).cloned());
        }
        let dev = &gpu.device;
        let u = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let mk = |label: &str, size: u64| crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some(label), size, usage: u, mapped_at_creation: false });
        let m = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("floatZ (exe encoding)"), source: wgpu::ShaderSource::Wgsl(CONVERT.into()) });
        let convert = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("floatZ (exe encoding)"),
            layout: None,
            vertex: wgpu::VertexState { module: &m, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &m, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::R32Float.into())] }),
            multiview_mask: None,
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        // (PostFxCBuffer: the exe sets none of it for this pass: zero.)
        let post_fx = mk("t7 motion PostFxCBuffer", 16);
        gpu.queue.write_buffer_p(&post_fx, 0, &[0u8; 16]);
        Ok(Self {
            gpu: gpu.clone(),
            pass,
            args,
            buffers: CodeBuffers::new(gpu),
            per_scene: mk("t7 motion PerSceneConsts", (REGISTERS * 16) as u64),
            post_fx,
            params: mk("MotionVectorParams", 15 * 16),
            convert_k: mk("floatZ scale", 16),
            convert,
            format,
            sized: None,
            prev: None,
            with_mb,
            blur_scale: 0.0,
        })
    }

    /// Whether `motion_vector_static_with_mb` was made (motion blur can run).
    pub fn has_blur(&self) -> bool {
        self.with_mb.is_some()
    }

    /// The view's motion blur scale from the next [`Motion::update`] (the
    /// exe's `0x141ddc6c0`: 0 off, else the blur dvar's scale).
    pub fn set_blur_scale(&mut self, scale: f32) {
        self.blur_scale = if self.with_mb.is_some() { scale.max(0.0) } else { 0.0 };
    }

    /// The blur velocity of the last recorded frame, when motion blur ran.
    pub fn blur_velocity(&self) -> Option<&wgpu::TextureView> {
        if self.blur_scale <= 0.0 {
            return None;
        }
        self.sized.as_ref()?.blur.as_ref().map(|b| &b.0.view)
    }

    /// Makes the targets for a `w` × `h` frame; `depth` the G-buffer depth
    /// (depth aspect).
    pub fn prepare(&mut self, w: u32, h: u32, depth: &wgpu::TextureView) -> Result<(), String> {
        let gpu = &self.gpu;
        let float_z = crate::gpu::target(gpu, "floatZ (exe encoding)", w, h, wgpu::TextureFormat::R32Float, wgpu::TextureUsages::empty());
        let velocity = std::array::from_fn(|i| crate::gpu::target(gpu, &format!("motion vectors (rt {:#x})", 0x16 + i), w, h, self.format, wgpu::TextureUsages::COPY_SRC));
        let convert = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("floatZ (exe encoding)"),
            layout: &self.convert.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) }, wgpu::BindGroupEntry { binding: 1, resource: self.convert_k.as_entire_binding() }],
        });
        let groups = self.pass.bind_with(gpu, &self.per_scene, &self.post_fx, &[(7, &self.params)], &self.buffers, |id| if id == 0x10 { Some(&float_z.view) } else { None }, |_| None)?;
        let vertices = crate::gpu::buffer(gpu, "motion vector triangle", &triangle(w, h), wgpu::BufferUsages::VERTEX);
        let blur = match &self.with_mb {
            Some(p) => {
                let t = crate::gpu::target(gpu, "motion blur velocity (role 0xb0e5c)", w, h, BLUR_VELOCITY_FORMAT, wgpu::TextureUsages::COPY_SRC);
                let g = p.bind_with(gpu, &self.per_scene, &self.post_fx, &[(7, &self.params)], &self.buffers, |id| if id == 0x10 { Some(&float_z.view) } else { None }, |_| None)?;
                Some((t, g))
            }
            None => None,
        };
        self.sized = Some(Sized { float_z, velocity, convert, groups, vertices, blur });
        let mut regs = [[0f32; 4]; REGISTERS];
        regs[..25].copy_from_slice(&per_scene(w, h));
        // (The program's vertex stage takes the code draw's transform as
        // `projectionMatrix`, registers 0..3.)
        let t: Vec<[f32; 4]> = regs[8..12].to_vec();
        regs[0..4].copy_from_slice(&t);
        let mut c = crate::code::CodeConsts::default();
        let (fw, fh) = (w as f32, h as f32);
        c.set(crate::lighting_globals::RENDER_TARGET_SIZE, vec![[fw, fh, 1.0 / fw, 1.0 / fh]]);
        // (upscaledTargetSize: `_with_mb` scales the velocity by
        // renderTargetSize / upscaledTargetSize; no upscaling here.)
        c.set(0x2, vec![[fw, fh, 1.0 / fw, 1.0 / fh]]);
        c.set(crate::lighting_globals::SUBPIXEL_OFFSET, vec![[0.0; 4]]);
        let mut bytes = bytemuck::cast_slice::<[f32; 4], u8>(&regs).to_vec();
        // (The vertex transform is not a camera matrix: keep it.)
        let args: Vec<_> = self.args.iter().filter(|a| a.u[0] & 0xff != 0xd9).cloned().collect();
        crate::lighting::fill_code(&mut bytes, &args, &c, &mut Vec::new());
        self.gpu.queue.write_buffer_p(&self.per_scene, 0, &bytes);
        Ok(())
    }

    /// The motion vectors of frame `frame` and of the frame before.
    pub fn velocity(&self, frame: u64) -> Option<(&wgpu::TextureView, &wgpu::TextureView)> {
        let s = self.sized.as_ref()?;
        Some((&s.velocity[(frame & 1) as usize].view, &s.velocity[((frame + 1) & 1) as usize].view))
    }

    /// Both motion-vector targets (by parity) for binding.
    pub fn targets(&self) -> Option<[&wgpu::TextureView; 2]> {
        let s = self.sized.as_ref()?;
        Some([&s.velocity[0].view, &s.velocity[1].view])
    }

    pub fn update(&mut self, camera: &Camera) {
        let prev = self.prev.replace(*camera).unwrap_or(*camera);
        let mut p = params(camera, &prev);
        // (12.w: the motion blur scale, exe `0x141ddc6c0`.)
        p[12][3] = self.blur_scale;
        self.gpu.queue.write_buffer_p(&self.params, 0, bytemuck::cast_slice(&p));
        self.gpu.queue.write_buffer_p(&self.convert_k, 0, bytemuck::cast_slice(&[63.0 / (64.0 * camera.near), 0.0, 0.0, 0.0f32]));
    }

    pub fn record(&self, enc: &mut wgpu::CommandEncoder, frame: u64) {
        let Some(s) = self.sized.as_ref() else { return };
        {
            let mut p = enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: Some("floatZ (exe encoding)"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &s.float_z.view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline_p(&self.convert);
            p.set_bind_group_p(0, &s.convert, &[]);
            p.draw_p(0..3, 0..1);
        }
        match (&self.with_mb, &s.blur) {
            (Some(p), Some((t, g))) if self.blur_scale > 0.0 => p.draw(enc, &[&s.velocity[(frame & 1) as usize].view, &t.view], g, &s.vertices),
            _ => self.pass.draw(enc, &[&s.velocity[(frame & 1) as usize].view], &s.groups, &s.vertices),
        }
    }
}
