//! The G-buffer's frustum culling on the GPU. Every G-buffer draw is
//! replayed from render bundles as an indirect draw whose arguments a
//! compute pass writes each frame: a world surface outside the view gets no
//! instances; an instanced draw (one static model's surface over all its
//! placements, one material's volume decals) draws only the instances whose
//! bounding spheres touch the view, their ids compacted into a region of
//! the instance id stream that the draw reads instead of the identity ids.
//! The bundles stay as they are (no CPU recording per frame); what is
//! culled drew no pixels anyway (the spheres hold the boxes, a tenth and 16
//! units larger), so the frame is the same.
//!
//! Before it, Black Ops III maps drew every surface and every placement
//! each frame (mp_sector: 3,320 world surfaces and 9,017 placements' finest
//! LOD), most of them behind the eye or off to the side.
//! `T7_NO_GPU_CULL=1`: the old direct draws. Needs
//! `INDIRECT_FIRST_INSTANCE` (off without it).

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use crate::scene::{NO_CULL, Scene};

/// Whether the culling is wanted (`T7_NO_GPU_CULL=1`: not).
pub fn wanted(device: &wgpu::Device) -> bool {
    !std::env::var("T7_NO_GPU_CULL").is_ok_and(|v| v.trim() != "0") && device.features().contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
}

/// The view's four side planes (inward normals, normalised) from a world to
/// clip matrix (any depth convention: the near / far planes are not used;
/// the side planes alone reject what is behind the eye).
pub fn planes(world_to_clip: glam::Mat4) -> [[f32; 4]; 4] {
    let t = world_to_clip.transpose();
    let (r0, r1, r3) = (t.x_axis, t.y_axis, t.w_axis);
    let mut out = [[0.0f32; 4]; 4];
    for (k, p) in [r3 + r0, r3 - r0, r3 + r1, r3 - r1].into_iter().enumerate() {
        let l = p.truncate().length();
        out[k] = if l > 0.0 && l.is_finite() { (p / l).to_array() } else { [0.0; 4] };
    }
    out
}

/// Whether a sphere (centre, radius; a negative radius: always) touches the
/// view of `planes` (the shader's test, for checks).
pub fn visible(planes: &[[f32; 4]; 4], s: [f32; 4]) -> bool {
    if s[3] < 0.0 {
        return true;
    }
    planes.iter().all(|p| p[0] * s[0] + p[1] * s[1] + p[2] * s[2] + p[3] >= -s[3])
}

/// A draw's own sphere: a world surface's bounds (group 3, non-empty);
/// anything else is never culled as a whole (an instanced draw's bounds are
/// its largest placement's, the sky and effect pools have none).
pub fn draw_sphere(d: &crate::scene::Draw) -> [f32; 4] {
    if d.group != 3 || d.mins == d.maxs || (0..3).any(|k| d.mins[k] > d.maxs[k]) {
        return [0.0, 0.0, 0.0, -1.0];
    }
    crate::scene::cull_sphere(d.mins, d.maxs)
}

const WGSL: &str = r#"
// lod0: distance scale, bias, size factor, smallest radius on screen;
// lod1: pixel scale, LOD pixel scale, LOD limit, (unused). eye.w: 1 when
// ranges pick LODs.
struct View { planes: array<vec4<f32>, 4>, elems: u32, draws: u32, pad0: u32, pad1: u32, eye: vec4<f32>, lod0: vec4<f32>, lod1: vec4<f32> };
struct DrawIn { index_count: u32, instances: u32, first_index: u32, base_vertex: i32, first_instance: u32, range: u32, pad0: u32, pad1: u32, sphere: vec4<f32> };
struct Args { index_count: u32, instance_count: u32, first_index: u32, base_vertex: i32, first_instance: u32 };
// A range: its first element, its first sub-range (one per LOD and one for
// every placement in view, or one when it picks no LOD), its LOD count (0:
// none), which LODs are built, the finest the view may take, the model's
// size and smallest radius for the view's kind, its LOD distances.
struct Range { first_elem: u32, sub: u32, count: u32, built: u32, view_finest: i32, size: f32, min_radius: f32, pad: u32, dist: array<vec4<f32>, 2> };
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> elems: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> elem_range: array<u32>;
@group(0) @binding(3) var<storage, read> range_base: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> ids: array<u32>;
@group(0) @binding(6) var<storage, read> draws: array<DrawIn>;
@group(0) @binding(7) var<storage, read_write> args: array<Args>;
@group(0) @binding(8) var<storage, read> ranges: array<Range>;

fn seen(s: vec4<f32>) -> bool {
    if (s.w < 0.0) { return true; }
    for (var k = 0u; k < 4u; k = k + 1u) {
        let p = view.planes[k];
        if (dot(p.xyz, s.xyz) + p.w < -s.w) { return false; }
    }
    return true;
}

// The exe's LOD pick for one distance (crate::lod::inner).
fn inner(d: f32, cull: f32, r: f32, sz: f32, rg: u32) -> i32 {
    let last = i32(ranges[rg].count) - 1;
    let x = d - r;
    if (0.0 - x >= 0.0 || x <= 0.0 || view.lod1.x == 0.0) { return last; }
    var fl = 0.0;
    if (view.lod0.w >= 0.0) { fl = select(view.lod0.w, cull, cull - view.lod0.w >= 0.0); }
    if (fl * d > view.lod1.x * r) { return -1; }
    let k2 = view.lod1.y * view.lod1.y;
    let x2 = x * x;
    var i = last;
    loop {
        if (i < 0) { break; }
        let t = ranges[rg].dist[u32(i) / 4u][u32(i) % 4u];
        if (sz * t * k2 > x2) { break; }
        i = i - 1;
    }
    return max(min(i, last), 0);
}

// The LOD drawn for element i of range rg (crate::lod::RangeLod::drawn),
// -1: none.
fn pick(i: u32, rg: u32) -> i32 {
    let p = elems[3u * i + 1u];
    let s = elems[3u * i + 2u].x;
    let e = view.eye.xyz - p.xyz;
    let dist = sqrt(max(e.z * e.z + (e.x * e.x + e.y * e.y), 0.0));
    let d = dist * view.lod0.x + view.lod0.y;
    let cull = ranges[rg].min_radius;
    let sz = ranges[rg].size * (s * view.lod0.z);
    var c = inner(d, cull, p.w, sz, rg);
    if (c == -1 && d != dist) { c = inner(dist, cull, p.w, sz, rg); }
    if (c > ranges[rg].view_finest) { c = ranges[rg].view_finest; }
    let n = i32(ranges[rg].count);
    c = min(c, max(n - 1 - i32(view.lod1.z), 0));
    if (c < 0) { return -1; }
    let built = ranges[rg].built;
    if ((built & (1u << u32(c))) != 0u) { return c; }
    for (var k = c + 1; k < n; k = k + 1) { if ((built & (1u << u32(k))) != 0u) { return k; } }
    for (var k = c - 1; k >= 0; k = k - 1) { if ((built & (1u << u32(k))) != 0u) { return k; } }
    return -1;
}

fn append(sub: u32, i: u32, first_elem: u32) {
    let rb = range_base[sub];
    let slot = atomicAdd(&counts[sub], 1u);
    // (rb.x: the range's first instance id, rb.y: the sub-range's region in
    // ids; the region holds the original ids of the instances seen.)
    ids[rb.y + slot] = rb.x + (i - first_elem);
}

@compute @workgroup_size(64)
fn cull_elems(@builtin(global_invocation_id) g: vec3<u32>) {
    let i = g.x + g.y * 65535u * 64u;
    if (i >= view.elems) { return; }
    if (!seen(elems[3u * i])) { return; }
    let r = elem_range[i];
    let rg = ranges[r];
    if (rg.count == 0u) {
        append(rg.sub, i, rg.first_elem);
        return;
    }
    append(rg.sub + rg.count, i, rg.first_elem);
    let lod = pick(i, r);
    if (lod >= 0) { append(rg.sub + u32(lod), i, rg.first_elem); }
}

@compute @workgroup_size(64)
fn write_args(@builtin(global_invocation_id) g: vec3<u32>) {
    let j = g.x + g.y * 65535u * 64u;
    if (j >= view.draws) { return; }
    let d = draws[j];
    var a: Args;
    a.index_count = d.index_count;
    a.first_index = d.first_index;
    a.base_vertex = d.base_vertex;
    if (d.range != 0xffffffffu) {
        a.instance_count = atomicLoad(&counts[d.range]);
        a.first_instance = range_base[d.range].y;
    } else {
        a.instance_count = select(0u, d.instances, seen(d.sphere));
        a.first_instance = d.first_instance;
    }
    args[j] = a;
}
"#;

const ARGS: u64 = 20;

/// The culling's buffers and programs for one list of ready draws.
pub struct Cull {
    view: wgpu::Buffer,
    counts: wgpu::Buffer,
    /// The instance id stream the culled draws read (identity ids first,
    /// then each range's compacted region).
    pub ids: wgpu::Buffer,
    /// Per ready draw, its indirect arguments (`ARGS` bytes each).
    pub args: wgpu::Buffer,
    group: wgpu::BindGroup,
    elems_pipe: wgpu::ComputePipeline,
    args_pipe: wgpu::ComputePipeline,
    elems: u32,
    draws: u32,
    /// What is culled (for the log).
    pub culled_ranges: usize,
    pub world_draws: usize,
    /// Of them, ranges whose draws are split by LOD (`crate::lod`).
    pub lod_ranges: usize,
}

fn dispatch(n: u32) -> (u32, u32) {
    let groups = n.div_ceil(64).max(1);
    if groups <= 65535 { (groups, 1) } else { (65535, groups.div_ceil(65535)) }
}

impl Cull {
    /// For `ready` (draw indices into `scene.draws`, in the bundles' order)
    /// over `identity` instance ids.
    pub fn new(device: &wgpu::Device, scene: &Scene, ready: &[usize], identity: u32) -> Self {
        use wgpu::util::DeviceExt;
        use wgpu::BufferUsages as U;
        // The ranges the ready draws use, each once; a range that picks
        // LODs has a sub-range per LOD and one for every placement in view.
        let mut used: Vec<u32> = ready.iter().map(|&d| scene.draws[d].cull).filter(|&c| c != NO_CULL).collect();
        used.sort_unstable();
        used.dedup();
        let mut local = std::collections::HashMap::new();
        let (mut elems, mut elem_range, mut range_base, mut ranges) = (Vec::<[f32; 4]>::new(), Vec::<u32>::new(), Vec::<[u32; 2]>::new(), Vec::<u8>::new());
        let mut region = identity;
        let mut lod_ranges = 0;
        for (k, &c) in used.iter().enumerate() {
            let r = &scene.cull_ranges[c as usize];
            let sub = range_base.len() as u32;
            let count = r.lod.as_ref().map_or(0, |l| u32::from(l.model.count));
            local.insert(c, (sub, count));
            let first_elem = (elems.len() / 3) as u32;
            for _ in 0..count.max(1) + u32::from(count > 0) {
                range_base.push([r.first, region]);
                region += r.spheres.len() as u32;
            }
            for (j, s) in r.spheres.iter().enumerate() {
                let p = r.lod.as_ref().and_then(|l| l.placements.get(j)).copied().unwrap_or_default();
                elems.extend_from_slice(&[*s, [p.point[0], p.point[1], p.point[2], p.radius], [p.scale, 0.0, 0.0, 0.0]]);
            }
            elem_range.extend(std::iter::repeat_n(k as u32, r.spheres.len()));
            let (built, finest, size, min_radius, dist) = match &r.lod {
                Some(l) => {
                    lod_ranges += 1;
                    (l.built, i32::from(l.model.view_finest), l.model.size_kind[0] * l.model.size, l.model.radius_kind[0] * l.model.min_radius, l.model.dist)
                }
                None => (0, 0, 0.0, 0.0, [0.0; crate::lod::MAX_LODS]),
            };
            let words: [u32; 8] = [first_elem, sub, count, built, finest as u32, size.to_bits(), min_radius.to_bits(), 0];
            ranges.extend_from_slice(bytemuck::cast_slice(&words));
            ranges.extend_from_slice(bytemuck::cast_slice(&dist));
        }
        let mut world_draws = 0;
        let mut draws: Vec<u8> = Vec::with_capacity(ready.len() * 48);
        for &di in ready {
            let d = &scene.draws[di];
            let range = match local.get(&d.cull) {
                Some(&(sub, 0)) => sub,
                Some(&(sub, count)) => sub + if d.lod == crate::scene::ANY_LOD { count } else { u32::from(d.lod).min(count - 1) },
                None => NO_CULL,
            };
            let sphere = if range == NO_CULL { draw_sphere(d) } else { [0.0, 0.0, 0.0, -1.0] };
            if range == NO_CULL && sphere[3] >= 0.0 {
                world_draws += 1;
            }
            let words: [u32; 8] = [d.index_count, d.instances, d.first_index, d.base_vertex as u32, d.instance, range, 0, 0];
            draws.extend_from_slice(bytemuck::cast_slice(&words));
            draws.extend_from_slice(bytemuck::cast_slice(&sphere));
        }
        // (Storage bindings may not be empty.)
        let nonempty = |v: Vec<u8>, min: usize| if v.len() < min { let mut v = v; v.resize(min, 0); v } else { v };
        let mut ids: Vec<u32> = (0..identity).collect();
        ids.resize(region as usize, 0);
        let buf = |label: &str, data: Vec<u8>, usage: U| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: &data, usage });
        let view = device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 cull view"), size: 128, usage: U::UNIFORM | U::COPY_DST, mapped_at_creation: false });
        let spheres_b = buf("t7 cull elements", nonempty(bytemuck::cast_slice(&elems).to_vec(), 48), U::STORAGE);
        let elem_b = buf("t7 cull element ranges", nonempty(bytemuck::cast_slice(&elem_range).to_vec(), 4), U::STORAGE);
        let base_b = buf("t7 cull range bases", nonempty(bytemuck::cast_slice(&range_base).to_vec(), 8), U::STORAGE);
        let first_b = buf("t7 cull ranges", nonempty(ranges, 64), U::STORAGE);
        let counts = device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 cull counts"), size: (4 * range_base.len().max(1)) as u64, usage: U::STORAGE | U::COPY_DST, mapped_at_creation: false });
        let ids_b = buf("t7 culled instance ids", nonempty(bytemuck::cast_slice(&ids).to_vec(), 4), U::STORAGE | U::VERTEX);
        let draws_b = buf("t7 cull draws", nonempty(draws, 48), U::STORAGE);
        // (Before the first frame's pass: every draw whole.)
        let mut init: Vec<u32> = Vec::with_capacity(ready.len() * 5);
        for &di in ready {
            let d = &scene.draws[di];
            init.extend_from_slice(&[d.index_count, d.instances, d.first_index, d.base_vertex as u32, d.instance]);
        }
        let args = buf("t7 cull indirect args", nonempty(bytemuck::cast_slice(&init).to_vec(), ARGS as usize), U::STORAGE | U::INDIRECT | U::COPY_SRC);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 cull"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let entry = |b: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let ro = wgpu::BufferBindingType::Storage { read_only: true };
        let rw = wgpu::BufferBindingType::Storage { read_only: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("t7 cull"),
            entries: &[entry(0, wgpu::BufferBindingType::Uniform), entry(1, ro), entry(2, ro), entry(3, ro), entry(4, rw), entry(5, rw), entry(6, ro), entry(7, rw), entry(8, ro)],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("t7 cull"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: spheres_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: elem_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: base_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: counts.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: ids_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: draws_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: args.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: first_b.as_entire_binding() },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("t7 cull"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipe = |e: &str| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("t7 cull"), layout: Some(&pl), module: &module, entry_point: Some(e), compilation_options: Default::default(), cache: crate::pipecache::get(&device).as_ref() });
        Self {
            view,
            counts,
            ids: ids_b,
            args,
            group,
            elems_pipe: pipe("cull_elems"),
            args_pipe: pipe("write_args"),
            elems: (elems.len() / 3) as u32,
            draws: ready.len() as u32,
            culled_ranges: used.len(),
            lod_ranges,
            world_draws,
        }
    }

    /// The view the next frame culls against: its world to clip matrix,
    /// the eye and the LOD context (`crate::lod`).
    pub fn set_view(&self, queue: &wgpu::Queue, world_to_clip: glam::Mat4, eye: [f32; 3], lod: &crate::lod::View) {
        let p = planes(world_to_clip);
        let mut words = [0u32; 32];
        let f: &[u32] = bytemuck::cast_slice(&p);
        words[..16].copy_from_slice(f);
        words[16] = self.elems;
        words[17] = self.draws;
        let tail = [eye[0], eye[1], eye[2], if self.lod_ranges > 0 { 1.0 } else { 0.0 }, lod.scale, lod.bias, lod.size, lod.min_radius, lod.px, lod.lod_px, lod.limit as f32, 0.0];
        words[20..].copy_from_slice(bytemuck::cast_slice(&tail));
        queue.write_buffer_p(&self.view, 0, bytemuck::cast_slice(&words));
    }

    /// The culling's passes (before the G-buffer pass that reads `args`).
    pub fn record(&self, enc: &mut wgpu::CommandEncoder) {
        enc.clear_buffer(&self.counts, 0, None);
        if self.elems > 0 {
            let mut p = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 cull instances"), timestamp_writes: None });
            p.set_pipeline_p(&self.elems_pipe);
            p.set_bind_group_p(0, &self.group, &[]);
            let (x, y) = dispatch(self.elems);
            p.dispatch_p(x, y, 1);
        }
        if self.draws > 0 {
            let mut p = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 cull draws"), timestamp_writes: None });
            p.set_pipeline_p(&self.args_pipe);
            p.set_bind_group_p(0, &self.group, &[]);
            let (x, y) = dispatch(self.draws);
            p.dispatch_p(x, y, 1);
        }
    }

    /// The byte offset of ready draw `j`'s arguments.
    pub fn offset(j: usize) -> u64 {
        j as u64 * ARGS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spheres_behind_and_beside_the_eye_are_out_ahead_in() {
        // Looking down +x from the origin, 90 degrees across.
        let view = glam::Mat4::look_at_rh(glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Z);
        let proj = glam::Mat4::perspective_infinite_reverse_rh(std::f32::consts::FRAC_PI_2, 16.0 / 9.0, 4.0);
        let p = planes(proj * view);
        assert!(visible(&p, [1000.0, 0.0, 0.0, 10.0]));
        assert!(!visible(&p, [-1000.0, 0.0, 0.0, 10.0]), "behind");
        assert!(!visible(&p, [100.0, 1000.0, 0.0, 10.0]), "far to the side");
        assert!(visible(&p, [-5.0, 0.0, 0.0, 10.0]), "around the eye");
        assert!(visible(&p, [-1000.0, 0.0, 0.0, -1.0]), "never culled");
        // A zero view culls nothing.
        assert!(visible(&[[0.0; 4]; 4], [-1000.0, 0.0, 0.0, 1.0]));
    }
}
