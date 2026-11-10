//! Reflection probes, as the lit pixel programs read them (`lmap_*`,
//! `lprobe_*`): the pixel's world position (its position about the eye plus
//! code 17, the eye) picks a cell of a 32 x 24 x 48 grid, whose two words
//! (t32, 8 bytes a cell) hold a bit per probe; each probe set there (t31,
//! 128 bytes each) is weighed front to back by how deep the pixel lies in
//! its box until the weight is spent, and its cube (code texture 37, a
//! cube array, at mip `5 - 5 gloss`) is sampled along the reflection,
//! corrected for the box (parallax), scaled to the pixel's own light by
//! the probe's luminance in that direction (t33, an L1 spherical harmonic
//! in half floats) clamped to code 151's xy. Code 141 (`saturate(gloss x +
//! y)`) blends the probes over the program's own fallback.
//!
//! A probe record (32 words): origin and cube layer (0-3; x as -1 bits: no
//! parallax), box centre and its three axes (4-15), half extents (16-18),
//! fade scales (19-21: one over the fade length), a turn of the cube
//! (quaternion xyz in 24, 22, 23, w in 25; w 1 none), the parallax box's
//! planes along each axis (26-31: -h, h). Record 64 holds the grid's
//! mapping: four columns taking the world position to (x/w, y/w, z) of the
//! grid (z is square-rooted), then the last bit word to scan (16).
//!
//! The map gives only the probes' origins and their cube array, so each
//! probe's box is a cube round its origin sized by its nearest neighbour,
//! smallest first, the grid a world-space one over x and y (one z layer),
//! and the luminance is measured from the cubes here. The values of codes
//! 141 and 151 are not proven. `BO2MP_IW7_REFLECTIONS=0`: none.

use render_h1::gpu::Gpu;

pub const GRID: [u32; 3] = [32, 24, 48];
pub const MAX_PROBES: usize = 64;
const RECORD_WORDS: usize = 32;
const SH_WORDS: usize = 8;

/// Code 141: the probes' blend over the fallback by gloss (always).
pub const BLEND: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
/// Code 151: the luminance match's clamp.
pub const MATCH_CLAMP: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

/// One probe's box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeBox {
    pub layer: u32,
    pub origin: [f32; 3],
    pub half: [f32; 3],
}

/// The probes' boxes, smallest first.
pub fn boxes(origins: &[[f32; 3]]) -> Vec<ProbeBox> {
    let mut out: Vec<ProbeBox> = origins
        .iter()
        .enumerate()
        .take(MAX_PROBES)
        .map(|(i, o)| {
            let near = origins
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, p)| ((p[0] - o[0]).powi(2) + (p[1] - o[1]).powi(2) + (p[2] - o[2]).powi(2)).sqrt())
                .fold(f32::MAX, f32::min);
            let h = if near == f32::MAX { 4096.0 } else { (near * 0.75).clamp(256.0, 3072.0) };
            ProbeBox { layer: i as u32, origin: *o, half: [h, h, h.max(512.0)] }
        })
        .collect();
    out.sort_by(|a, b| a.half[0].total_cmp(&b.half[0]));
    out
}

impl ProbeBox {
    pub fn record(&self) -> [u32; RECORD_WORDS] {
        let mut r = [0u32; RECORD_WORDS];
        let mut f = |i: usize, v: f32| r[i] = v.to_bits();
        let [x, y, z] = self.origin;
        let [hx, hy, hz] = self.half;
        for (i, v) in [x, y, z].into_iter().enumerate() {
            f(i, v);
            f(4 + i, v);
            f(16 + i, self.half[i]);
            // (Fade over the outer quarter.)
            f(19 + i, 1.0 / (0.25 * self.half[i]));
        }
        for (i, v) in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0].into_iter().enumerate() {
            f(7 + i, v);
        }
        f(25, 1.0);
        for (i, v) in [-hx, hx, -hy, hy, -hz, hz].into_iter().enumerate() {
            f(26 + i, v);
        }
        r[3] = self.layer;
        r
    }
}

/// The world-space grid over the boxes: (min x, min y, size x, size y).
fn grid_rect(boxes: &[ProbeBox]) -> [f32; 4] {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for b in boxes {
        for k in 0..2 {
            lo[k] = lo[k].min(b.origin[k] - b.half[k]);
            hi[k] = hi[k].max(b.origin[k] + b.half[k]);
        }
    }
    [lo[0], lo[1], (hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0)]
}

/// t31 (the records and the grid's), t32 (the cells).
pub fn buffers(boxes: &[ProbeBox]) -> (Vec<u32>, Vec<u32>) {
    let mut records = vec![0u32; (MAX_PROBES + 1) * RECORD_WORDS];
    for (i, b) in boxes.iter().enumerate() {
        records[i * RECORD_WORDS..(i + 1) * RECORD_WORDS].copy_from_slice(&b.record());
    }
    let [mx, my, sx, sy] = grid_rect(boxes);
    let cols: [f32; 16] = [1.0 / sx, 0.0, 0.0, 0.0, 0.0, 1.0 / sy, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -mx / sx, -my / sy, 0.0, 1.0];
    let g = MAX_PROBES * RECORD_WORDS;
    for (i, c) in cols.into_iter().enumerate() {
        records[g + i] = c.to_bits();
    }
    records[g + 16] = (boxes.len().max(1) as u32 - 1) / 32;
    let (gx, gy) = (GRID[0] as usize, GRID[1] as usize);
    let mut cells = vec![0u32; gx * gy * 2];
    for cy in 0..gy {
        for cx in 0..gx {
            let (x0, y0) = (mx + sx * cx as f32 / gx as f32, my + sy * cy as f32 / gy as f32);
            let (x1, y1) = (x0 + sx / gx as f32, y0 + sy / gy as f32);
            for (i, b) in boxes.iter().enumerate() {
                let meets = b.origin[0] + b.half[0] >= x0 && b.origin[0] - b.half[0] <= x1 && b.origin[1] + b.half[1] >= y0 && b.origin[1] - b.half[1] <= y1;
                if meets {
                    cells[(cy * gx + cx) * 2 + i / 32] |= 1 << (i % 32);
                }
            }
        }
    }
    (records, cells)
}

const SH: &str = r"
@group(0) @binding(0) var cubes: texture_cube_array<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<storage, read_write> out: array<u32>;
const N: u32 = 256u;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let layer = id.x;
    if (layer >= textureNumLayers(cubes)) { return; }
    let mip = f32(textureNumLevels(cubes)) - 3.0;
    var c = vec4<f32>(0.0);
    for (var i = 0u; i < N; i++) {
        // (A Fibonacci sphere.)
        let z = 1.0 - (f32(i) + 0.5) * 2.0 / f32(N);
        let r = sqrt(max(1.0 - z * z, 0.0));
        let a = f32(i) * 2.39996323;
        let d = vec3<f32>(r * cos(a), r * sin(a), z);
        let rgb = textureSampleLevel(cubes, smp, d, i32(layer), max(mip, 0.0)).rgb;
        let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        c += lum * vec4<f32>(-0.4886 * d.x, -0.4886 * d.y, 0.2821, 0.4886 * d.z);
    }
    c *= 12.566371 / f32(N);
    out[layer * 8u] = pack2x16float(vec2<f32>(c.z, c.y));
    out[layer * 8u + 1u] = pack2x16float(vec2<f32>(c.w, c.x));
}
";

/// The probe buffers (t31, t32, t33) and the cube array.
pub struct Reflections {
    pub records: wgpu::Buffer,
    pub cells: wgpu::Buffer,
    pub sh: wgpu::Buffer,
    pub cubes: Option<wgpu::TextureView>,
}

impl Reflections {
    /// `cubes`: the map's probe cube array, if loaded (none: off).
    pub fn new(g: &Gpu, origins: &[[f32; 3]], cubes: Option<wgpu::TextureView>) -> Self {
        use wgpu::util::DeviceExt;
        let cubes = cubes.filter(|_| !std::env::var("BO2MP_IW7_REFLECTIONS").is_ok_and(|v| v.trim() == "0"));
        let b = boxes(origins);
        let (records, cells) = buffers(&b);
        let init = |label: &str, data: &[u32]| g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(data), usage: wgpu::BufferUsages::STORAGE });
        let records = init("iw7 probe records", &records);
        let cells = init("iw7 probe cells", &cells);
        let layers = origins.len().max(1);
        let sh = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 probe luminance"), size: (layers * SH_WORDS * 4) as u64, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        if let Some(view) = cubes.as_ref() {
            let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 probe luminance"), source: wgpu::ShaderSource::Wgsl(SH.into()) });
            let pipe = g.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("iw7 probe luminance"), layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
            let smp = g.device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, ..Default::default() });
            let group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 probe luminance"),
                layout: &pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&smp) },
                    wgpu::BindGroupEntry { binding: 2, resource: sh.as_entire_binding() },
                ],
            });
            let mut enc = g.device.create_command_encoder(&Default::default());
            {
                let mut cp = enc.begin_compute_pass(&Default::default());
                cp.set_pipeline(&pipe);
                cp.set_bind_group(0, &group, &[]);
                cp.dispatch_workgroups(layers as u32, 1, 1);
            }
            g.queue.submit([enc.finish()]);
        }
        Self { records, cells, sh, cubes }
    }

    pub fn on(&self) -> bool {
        self.cubes.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_probe_is_listed_in_the_cell_under_it() {
        let o = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [5000.0, 3000.0, 0.0]];
        let b = boxes(&o);
        let (records, cells) = buffers(&b);
        let g = MAX_PROBES * RECORD_WORDS;
        let col = |c: usize| (0..4).map(|k| f32::from_bits(records[g + c * 4 + k])).collect::<Vec<_>>();
        for (i, p) in b.iter().enumerate() {
            // The programs' mapping: x * col0 + y * col1 + z * col2 + col3.
            let q: Vec<f32> = (0..4).map(|k| p.origin[0] * col(0)[k] + p.origin[1] * col(1)[k] + p.origin[2] * col(2)[k] + col(3)[k]).collect();
            let cx = ((q[0] / q[3]) * 32.0).floor().clamp(0.0, 31.0) as usize;
            let cy = ((q[1] / q[3]) * 24.0).floor().clamp(0.0, 23.0) as usize;
            assert!(cells[(cy * 32 + cx) * 2 + i / 32] & (1 << (i % 32)) != 0, "probe {i}");
            assert_eq!(records[i * RECORD_WORDS + 3], p.layer);
        }
        assert_eq!(records[g + 16], 0);
        // Smallest first.
        assert!(b.windows(2).all(|w| w[0].half[0] <= w[1].half[0]));
    }
}
