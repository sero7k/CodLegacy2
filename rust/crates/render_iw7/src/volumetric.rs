//! Volumetric fog: the froxel volumes the lit pixel programs read last
//! (code textures 51, in-scattered light, and 52, transmittance, sampled
//! at the pixel's screen uv and a depth coordinate), and the depth mapping
//! they read it with (codes 144 and 145):
//! `z = 0.25 * sum_i saturate(s_i * c145_i)` with `s_0 = w - 6` (w the
//! view depth) and `s_{i+1} = s_i - c144_i`: four linear segments, each a
//! quarter of the volume ([`SEGMENTS`] long). The programs then take
//! `colour * transmittance + inscatter`.
//!
//! The game fills the volumes with its own compute programs (in
//! `code_post_gfx`, not read here); this fills them with one of our own
//! from the vision's `volumetric*` settings: air density, albedo, the sun's
//! brightness and anisotropy (Henyey-Greenstein) lit through the sun
//! cascades (`screen`), and an ambient term from the fog colour, marched
//! front to back per froxel column. The air density's unit (here per
//! thousand units) is not proven. `BO2MP_IW7_VOLUMETRIC=0`: none.

use fastfile_iw7::light::vision_value;
use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;

/// The froxel grid.
pub const SIZE: [u32; 3] = [160, 90, 64];
/// The four depth segments' lengths (world units).
pub const SEGMENTS: [f32; 4] = [256.0, 768.0, 2048.0, 5120.0];
/// What the vision's air density is per (the unproven unit).
pub const DENSITY_UNIT: f32 = 1000.0;

/// The vision's volumetric settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub density: f32,
    pub albedo: [f32; 3],
    pub sun_brightness: f32,
    pub ambient_brightness: f32,
    pub anisotropy: f32,
    pub ambient: [f32; 3],
}

impl Settings {
    pub fn from_vision(text: &str) -> Option<Self> {
        let num = |k: &str| vision_value(text, k).and_then(|v| v.split_whitespace().next()?.parse::<f32>().ok());
        let three = |k: &str| {
            let v: Vec<f32> = vision_value(text, k)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (v.len() >= 3).then(|| [v[0], v[1], v[2]])
        };
        let density = num("volumetricAirDensity").filter(|d| *d > 0.0)?;
        let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let fog = three("fogColor").unwrap_or([0.5; 3]).map(lin);
        let fog_i = num("fogColorIntensity").unwrap_or(1.0);
        Some(Self {
            density,
            albedo: three("volumetricAlbedo").unwrap_or([1.0; 3]),
            sun_brightness: num("volumetricSunBrightness").unwrap_or(1.0),
            ambient_brightness: num("volumetricAmbientBrightness").unwrap_or(0.0),
            anisotropy: num("volumetricSunAnisotropy").unwrap_or(0.0).clamp(-0.95, 0.95),
            ambient: fog.map(|c| c * fog_i),
        })
    }
}

/// The depth mapping's code rows: 144 the segment lengths, 145 their
/// reciprocals.
pub fn mapping_rows() -> ([f32; 4], [f32; 4]) {
    (SEGMENTS, SEGMENTS.map(|l| 1.0 / l))
}

/// The view depth at froxel depth coordinate `z` (0..1).
pub fn depth_at(z: f32) -> f32 {
    let mut w = 6.0;
    let mut left = (z * 4.0).clamp(0.0, 4.0);
    for l in SEGMENTS {
        let t = left.min(1.0);
        w += t * l;
        left -= t;
        if left <= 0.0 {
            break;
        }
    }
    w
}

const MARCH: &str = r"
struct U {
    inv_vp: mat4x4<f32>,
    fwd: vec4<f32>,
    sun0: mat4x4<f32>,
    c0: vec4<f32>,
    sun1: mat4x4<f32>,
    c1: vec4<f32>,
    // sun toward (xyz), density per unit (w)
    sun: vec4<f32>,
    // sun colour times brightness (xyz), anisotropy (w)
    sun_c: vec4<f32>,
    // albedo (xyz), ambient brightness (w)
    albedo: vec4<f32>,
    // ambient colour (xyz), segment count
    ambient: vec4<f32>,
    // the four segments' lengths
    seg: vec4<f32>,
    eye: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var sh0: texture_depth_2d;
@group(0) @binding(2) var sh1: texture_depth_2d;
@group(0) @binding(3) var cmp: sampler_comparison;
@group(0) @binding(4) var inscatter: texture_storage_3d<rgba16float, write>;
@group(0) @binding(5) var transmit: texture_storage_3d<rgba16float, write>;

fn depth_at(z: f32) -> f32 {
    var w = 6.0;
    var left = clamp(z * 4.0, 0.0, 4.0);
    for (var i = 0; i < 4; i++) {
        let t = min(left, 1.0);
        w += t * u.seg[i];
        left -= t;
    }
    return w;
}

fn lit(m: mat4x4<f32>, c: vec4<f32>, world: vec3<f32>, which: i32) -> vec2<f32> {
    let s = m * vec4<f32>(world - c.xyz, 1.0);
    let uv = s.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || s.z <= 0.0 || s.z >= 1.0) {
        return vec2<f32>(1.0, 0.0);
    }
    if (which == 0) {
        return vec2<f32>(textureSampleCompareLevel(sh0, cmp, uv, s.z + c.w), 1.0);
    }
    return vec2<f32>(textureSampleCompareLevel(sh1, cmp, uv, s.z + c.w), 1.0);
}

fn sun_visible(world: vec3<f32>) -> f32 {
    let a = lit(u.sun0, u.c0, world, 0);
    if (a.y > 0.5) { return a.x; }
    return lit(u.sun1, u.c1, world, 1).x;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(inscatter);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size.xy);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    let p = u.inv_vp * ndc;
    let dir = normalize(p.xyz / p.w);
    let along = max(dot(dir, u.fwd.xyz), 1e-3);
    // Henyey-Greenstein toward the sun.
    let g = u.sun_c.w;
    let cos_t = dot(dir, u.sun.xyz);
    let phase = (1.0 - g * g) / (12.566371 * pow(max(1.0 + g * g - 2.0 * g * cos_t, 1e-4), 1.5));
    var acc = vec3<f32>(0.0);
    var trans = 1.0;
    var prev = 0.0;
    for (var k = 0u; k < size.z; k++) {
        let z = (f32(k) + 0.5) / f32(size.z);
        let w = depth_at(z);
        let dist = w / along;
        let step = max(dist - prev, 0.0);
        prev = dist;
        let world = dir * dist + u.eye.xyz;
        let sigma = u.sun.w;
        var vis = 1.0;
        if (u.eye.w > 0.5) { vis = sun_visible(world); }
        let light = u.albedo.xyz * (u.sun_c.xyz * phase * vis + u.ambient.xyz * u.albedo.w);
        let t_step = exp(-sigma * step);
        // (In-scattered light over the step, energy conserving.)
        acc += trans * light * (1.0 - t_step);
        trans *= t_step;
        textureStore(inscatter, vec3<u32>(id.xy, k), vec4<f32>(acc, 1.0));
        textureStore(transmit, vec3<u32>(id.xy, k), vec4<f32>(trans, trans, trans, 1.0));
    }
}
";

/// The froxel volumes and their compute pass.
pub struct Volumetric {
    gpu: Gpu,
    pub settings: Option<Settings>,
    pub inscatter: (wgpu::Texture, wgpu::TextureView),
    pub transmit: (wgpu::Texture, wgpu::TextureView),
    pipe: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    cmp: wgpu::Sampler,
}

impl Volumetric {
    /// `cascades`: the sun's cascade depth views (`screen`).
    pub fn new(g: &Gpu, settings: Option<Settings>, cascades: [&wgpu::TextureView; 2]) -> Self {
        let settings = settings.filter(|_| !std::env::var("BO2MP_IW7_VOLUMETRIC").is_ok_and(|v| v.trim() == "0"));
        // (Half floats: 0 or 1.)
        let vol = |label: &str, fill: f32| {
            let t = g.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: SIZE[2] },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            // (Start clear: nothing in-scattered, everything let through.)
            let h: [u8; 2] = if fill == 0.0 { [0, 0] } else { 0x3c00u16.to_le_bytes() };
            let texel: Vec<u8> = (0..4).flat_map(|_| h).collect();
            let n = (SIZE[0] * SIZE[1] * SIZE[2]) as usize;
            let data: Vec<u8> = (0..n).flat_map(|_| texel.clone()).collect();
            g.queue.write_texture(
                t.as_image_copy(),
                &data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE[0] * 8), rows_per_image: Some(SIZE[1]) },
                wgpu::Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: SIZE[2] },
            );
            let v = t.create_view(&Default::default());
            (t, v)
        };
        let inscatter = vol("iw7 volumetric inscatter", 0.0);
        let transmit = vol("iw7 volumetric transmittance", 1.0);
        let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 volumetric"), source: wgpu::ShaderSource::Wgsl(MARCH.into()) });
        let pipe = g.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("iw7 volumetric"), layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
        let uniform = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 volumetric"), size: 4 * (16 * 3 + 4 * 9), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let cmp = g.device.create_sampler(&wgpu::SamplerDescriptor { compare: Some(wgpu::CompareFunction::GreaterEqual), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let group = Self::make_group(g, &pipe, &uniform, cascades, &cmp, &inscatter.1, &transmit.1);
        Self { gpu: g.clone(), settings, inscatter, transmit, pipe, group, uniform, cmp }
    }

    fn make_group(g: &Gpu, pipe: &wgpu::ComputePipeline, uniform: &wgpu::Buffer, cascades: [&wgpu::TextureView; 2], cmp: &wgpu::Sampler, inscatter: &wgpu::TextureView, transmit: &wgpu::TextureView) -> wgpu::BindGroup {
        g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw7 volumetric"),
            layout: &pipe.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(cascades[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(cascades[1]) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(cmp) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(inscatter) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(transmit) },
            ],
        })
    }

    /// Takes the sun cascades again (after their size changed).
    pub fn rebind(&mut self, cascades: [&wgpu::TextureView; 2]) {
        self.group = Self::make_group(&self.gpu, &self.pipe, &self.uniform, cascades, &self.cmp, &self.inscatter.1, &self.transmit.1);
    }

    /// Fills the volumes for a frame (after the sun cascades are drawn).
    /// `cascades`: each cascade's view-projection about its centre, the
    /// centre and its depth bias (`screen`).
    #[allow(clippy::too_many_arguments)]
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, view_projection: Mat4, eye: Vec3, forward: Vec3, sun: Option<(Vec3, [f32; 3])>, cascades: [(Mat4, Vec3, f32); 2], shadowed: bool) -> u64 {
        let Some(s) = self.settings else { return 0 };
        let (toward, colour) = sun.unwrap_or((Vec3::Z, [0.0; 3]));
        let mut f = crate::cull::Pack::<96>::new();
        f.extend_from_slice(&view_projection.inverse().to_cols_array());
        f.extend_from_slice(&[forward.x, forward.y, forward.z, 0.0]);
        for (vp, c, bias) in cascades {
            f.extend_from_slice(&vp.to_cols_array());
            f.extend_from_slice(&[c.x, c.y, c.z, bias]);
        }
        let t = toward.normalize_or(Vec3::Z);
        f.extend_from_slice(&[t.x, t.y, t.z, s.density / DENSITY_UNIT]);
        f.extend_from_slice(&[colour[0] * s.sun_brightness, colour[1] * s.sun_brightness, colour[2] * s.sun_brightness, s.anisotropy]);
        f.extend_from_slice(&[s.albedo[0], s.albedo[1], s.albedo[2], s.ambient_brightness]);
        f.extend_from_slice(&[s.ambient[0], s.ambient[1], s.ambient[2], 4.0]);
        f.extend_from_slice(&SEGMENTS);
        f.extend_from_slice(&[eye.x, eye.y, eye.z, if shadowed { 1.0 } else { 0.0 }]);
        self.gpu.queue.write_buffer(&self.uniform, 0, f.bytes());
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("iw7 volumetric"), timestamp_writes: None });
        cp.set_pipeline(&self.pipe);
        cp.set_bind_group(0, &self.group, &[]);
        cp.dispatch_workgroups(SIZE[0].div_ceil(8), SIZE[1].div_ceil(8), 1);
        (f.len() * 4) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_depth_mapping_inverts_the_programs() {
        let (c144, c145) = mapping_rows();
        let z_of = |w: f32| {
            let mut s = w - 6.0;
            let mut z = 0.0;
            for i in 0..4 {
                z += (s * c145[i]).clamp(0.0, 1.0) * 0.25;
                s -= c144[i];
            }
            z
        };
        for z in [0.1f32, 0.3, 0.6, 0.9] {
            assert!((z_of(depth_at(z)) - z).abs() < 1e-4, "{z}");
        }
    }
}
