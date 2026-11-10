//! Spot and omni light shadows as BlackOps3.exe draws them for a view: the
//! light packer sets the shadowed lights aside, ranks them by the light
//! they throw into the view, gives the first few a tile of
//! `gSpotShadowmapArray` and a 15-vec4 shadow record after the light
//! records; the light's own record names its shadow record (flags word,
//! bits 20..23, 1-based) and the deferred lighting programs sample the
//! tile through it (8-tap comparison kernel, cubed).
//!
//! What is the exe's (addresses in `docs/exe_notes/T7_RENDER_GAPS.md`):
//! the eligible lights (spot or omni, shadow state not 1), the ranking (the
//! light's brightness at the corners of a box, then the distance), at most
//! four a view, an omni light drawn as a 90 degree spot looking at the
//! box's centre, the fit of the light's projection to the box (uv scale and
//! offset, the depth range), the tile allocator (an 8 x 8 grid of cells per
//! layer in Morton order, four tile sizes, the least recently used tile not
//! used this frame evicted), the record's words and the casters' matrix
//! (the light's projection times the fit).
//!
//! What is not: the exe's box is the bounds of the view's visible cells
//! (a list not traced here); the ranking takes the light's own box clipped
//! to the view's, and the fit the light's own box clipped to the light's
//! frustum (so a tile is drawn once while its light keeps it: the world
//! drawn here does not move). The casters are the opaque G-buffer surfaces
//! and static models (finest LOD) drawn depth only; cut-out and blended
//! materials do not cast. The view model's own set (bits 16..19) and the
//! static cached slots (shadow state 3's cube and spot slots) are not
//! drawn.

use glam::{Mat4, Vec3, Vec4};

use crate::profile::{EncExt, PassExt, QueueExt};

/// Shadow records a frame can hold (the exe's counter stops at 16).
pub const MAX_RECORDS: usize = 16;
/// Shadowed lights a view takes (the exe's 4; 8 only in a mode not used
/// in multiplayer).
pub const EXE_PER_VIEW: usize = 4;
/// The tile sizes: cells taken (mask), positions in a layer, size as a
/// fraction of the layer, and the step between positions in cells.
const TIER_MASK: [u64; 4] = [u64::MAX, 0xffff, 0xf, 1];
const TIER_POSITIONS: [u32; 4] = [1, 4, 16, 64];
const TIER_SCALE: [f32; 4] = [1.0, 0.5, 0.25, 0.125];
const TIER_STEP: [u32; 4] = [64, 16, 4, 1];
/// The exe's default tile sizes for spot and omni lights.
pub const SPOT_TIER: usize = 3;
pub const OMNI_TIER: usize = 2;
/// The omni light's cone (half angle).
const OMNI_HALF_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// `BO2MP_T7_SPOT_SHADOWS=0`: no local light shadows (lit as before).
pub fn enabled() -> bool {
    !std::env::var("BO2MP_T7_SPOT_SHADOWS").is_ok_and(|v| v.trim() == "0")
}

/// `BO2MP_T7_SHADOW_FILTER=0`: the comparison kernel's switch
/// (`LightingGlobals` word 58.y) left at 0 as before, one tap per shadow
/// lookup (the exe's default is the 8-tap kernel, sun cascades too).
pub fn filter_on() -> bool {
    !std::env::var("BO2MP_T7_SHADOW_FILTER").is_ok_and(|v| v.trim() == "0")
}

fn f(r: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn u(r: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn v3(r: &[u8], at: usize) -> Vec3 {
    Vec3::new(f(r, at), f(r, at + 4), f(r, at + 8))
}

/// A cell position's (x, y) in the 8 x 8 grid: the even and odd bits.
pub fn morton(k: u32) -> (u32, u32) {
    let (mut x, mut y) = (0, 0);
    for b in 0..16 {
        x |= ((k >> (2 * b)) & 1) << b;
        y |= ((k >> (2 * b + 1)) & 1) << b;
    }
    (x, y)
}

/// Whether the packer sets the light aside for a shadow: spot or omni, a
/// shadow state other than 1, and not the kind the exe skips (+0x98 = 2
/// with +0x89 set).
pub fn eligible(r: &[u8]) -> bool {
    let ty = u(r, 0x40);
    (ty == 2 || ty == 4) && u(r, 0x84) != 1 && !(u(r, 0x98) == 2 && r[0x89] != 0)
}

/// The light's brightness at the box's corners, the highest (the ranking):
/// its colour's length, the inverse-square falloff (+0x54 the radius it
/// holds full), the near and far smoothsteps from `atten` (the
/// intermediate's words 17..20) along the axis.
pub fn priority(r: &[u8], atten: [f32; 4], lo: Vec3, hi: Vec3) -> f32 {
    let o = v3(r, 0x48);
    let dir = v3(r, 0x9c);
    let colour = v3(r, 0x58).length();
    let r2 = f(r, 0x54) * f(r, 0x54);
    let pull = f(r, 0x12c);
    let mut best = 0.0f32;
    for c in 0..8 {
        let p = Vec3::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z });
        let t = pull - (dir.dot(p) - dir.dot(o));
        let fall = (r2 / (p - o).length_squared()).clamp(0.0, 1.0);
        let a = (atten[0] * t + atten[1]).clamp(0.0, 1.0);
        let b = (atten[2] * t + atten[3]).clamp(0.0, 1.0);
        let v = colour * (3.0 - 2.0 * a) * a * a * fall * (3.0 - 2.0 * b) * b * b;
        best = best.max(v);
    }
    best
}

/// The fit of a light's projection to a box (`m` world to the light's
/// clip, column vectors): the uv scale and the uv of the box's corner, the
/// depth range (z/w, reversed: the nearest is the largest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
    pub depth: [f32; 2],
}

impl Fit {
    /// The casters' matrix: the light's projection times the fit (x and y
    /// to the tile, depth to 0..1 over the range).
    pub fn crop(&self) -> Mat4 {
        let k = self.depth[1] - self.depth[0];
        let (sx, sy) = (self.scale[0], self.scale[1]);
        let (ox, oy) = (self.offset[0], self.offset[1]);
        // Rows for row vectors (the exe's), so these are the columns here.
        Mat4::from_cols(
            Vec4::new(sx * k, 0.0, 0.0, ((1.0 - 2.0 * ox) * sx - 1.0) * k),
            Vec4::new(0.0, sy * k, 0.0, ((2.0 * oy - 1.0) * sy + 1.0) * k),
            Vec4::new(0.0, 0.0, 1.0, -self.depth[0]),
            Vec4::new(0.0, 0.0, 0.0, k),
        )
        .transpose()
    }
}

/// The fit over `corners`. `clamp`: the uv range kept inside the light's
/// own square (a corner outside it is outside the light's frustum, which
/// lights nothing; the exe's fit leaves it, spending the tile on it).
pub fn fit(m: &Mat4, corners: &[Vec3; 8], clamp: bool) -> Option<Fit> {
    let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    let (mut d0, mut d1) = (f32::MAX, f32::MIN);
    let mut n = 0;
    for c in corners {
        let p = *m * c.extend(1.0);
        if p.z > p.w {
            continue;
        }
        n += 1;
        let d = p.z / p.w;
        let (x, y) = (p.x / p.w, p.y / p.w);
        let (u, v) = (x * 0.5 + 0.5, 0.5 - y * 0.5);
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
        d0 = d0.min(d);
        d1 = d1.max(d);
    }
    if n == 0 {
        return None;
    }
    if n < 8 {
        d1 = 1.0;
    }
    if clamp {
        (u0, u1, v0, v1) = (u0.max(0.0), u1.min(1.0), v0.max(0.0), v1.min(1.0));
        if u0 >= u1 || v0 >= v1 {
            return None;
        }
    }
    const EPS: f32 = 1.526_624_3e-5;
    let sx = 1.0 / (u1 - u0).max(EPS);
    let sy = 1.0 / (v1 - v0).max(EPS);
    if d1 <= 0.0 {
        return None;
    }
    let offset = [(u0 + u1) * 0.5 - 0.5 / sx, (v0 + v1) * 0.5 - 0.5 / sy];
    Some(Fit { scale: [sx, sy], offset, depth: [d0, d1] })
}

/// The light's projection as the exe builds it for a spot (view at the
/// origin pulled back along the back direction, a perspective of the
/// cone): `axis` the direction it looks, `cos` the cone's half angle's
/// cosine, `near`/`far` the planes' distances. Reversed depth (near 1).
pub fn spot_matrix(origin: Vec3, axis: Vec3, cos: f32, near: f32, far: f32) -> Mat4 {
    let z = axis.normalize();
    let helper = if z.z.abs() < 0.999 { Vec3::Z } else { Vec3::X };
    let x = helper.cross(z).normalize();
    let y = z.cross(x);
    let view = Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), Vec4::W).transpose() * Mat4::from_translation(-origin);
    let tan = (1.0 - cos * cos).max(0.0).sqrt() / cos;
    let n = near.max(1.0);
    let fa = far.max(n + 1.0);
    let proj = Mat4::from_cols(
        Vec4::new(1.0 / tan, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0 / tan, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -n / (fa - n), 1.0),
        Vec4::new(0.0, 0.0, n * fa / (fa - n), 0.0),
    );
    proj * view
}

/// The spot light's own projection (the light's first 64 bytes, rows for
/// row vectors), as a column-vector matrix.
pub fn light_matrix(r: &[u8]) -> Mat4 {
    Mat4::from_cols_array(&std::array::from_fn(|k| f(r, 4 * k)))
}

/// The omni light's: a 90 degree spot at its origin looking at `target`,
/// its planes the light's (+0x68 near, +0x6c far).
pub fn omni_matrix(r: &[u8], target: Vec3) -> Mat4 {
    let o = v3(r, 0x48);
    let axis = target - o;
    let axis = if axis.length_squared() > 0.0 { axis } else { Vec3::NEG_Z };
    let near = f(r, 0x68).max(1.0);
    let far = near + (f(r, 0x6c) - f(r, 0x68));
    spot_matrix(o, axis, OMNI_HALF_ANGLE.cos(), near, far)
}

/// The light's box (its +0x1f0..+0x204 bounds).
pub fn light_box(r: &[u8]) -> (Vec3, Vec3) {
    (v3(r, 0x1f0), v3(r, 0x1fc))
}

fn corners(lo: Vec3, hi: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|c| Vec3::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z }))
}

/// The shadow record (60 words) the lighting reads for a light: its
/// projection (eye relative, words 17..32 its columns' dot vectors), the
/// tile (layer in word 7's low half, the uv scale and offset in the layer
/// at 33..36), the fit (37..42) and the kernel radius in layer uv (13).
pub fn record(m_rel: &Mat4, layer: u32, tile: [f32; 4], fit: &Fit, radius: f32) -> [u32; 60] {
    let mut o = [0u32; 60];
    o[7] = layer & 0xffff;
    o[13] = radius.to_bits();
    for j in 0..4 {
        let row = m_rel.row(j);
        for i in 0..4 {
            o[17 + 4 * j + i] = row[i].to_bits();
        }
    }
    for k in 0..4 {
        o[33 + k] = tile[k].to_bits();
    }
    o[37] = fit.scale[0].to_bits();
    o[38] = fit.scale[1].to_bits();
    o[39] = (-fit.offset[0]).to_bits();
    o[40] = (-fit.offset[1]).to_bits();
    o[41] = (-fit.depth[0]).to_bits();
    o[42] = (fit.depth[1] - fit.depth[0]).to_bits();
    o
}

/// One tile of the atlas.
#[derive(Clone, Copy, Debug)]
struct Slot {
    key: u32,
    layer: u32,
    mask: u64,
    tier: usize,
    /// (scale, x offset, scale, y offset) in layer uv.
    params: [f32; 4],
    used: u64,
}

/// The tile allocator: per layer a mask of the 64 cells taken.
pub struct Atlas {
    layers: Vec<u64>,
    slots: Vec<Slot>,
}

impl Atlas {
    pub fn new(layers: u32) -> Self {
        Self { layers: vec![0; layers.max(1) as usize], slots: Vec::new() }
    }

    fn alloc(&mut self, key: u32, tier: usize, frame: u64) -> Option<usize> {
        for (layer, taken) in self.layers.iter_mut().enumerate() {
            for k in 0..TIER_POSITIONS[tier] {
                let mask = TIER_MASK[tier] << (k * TIER_STEP[tier]);
                if mask & *taken == 0 {
                    *taken |= mask;
                    let (x, y) = morton(k);
                    let s = TIER_SCALE[tier];
                    self.slots.push(Slot { key, layer: layer as u32, mask, tier, params: [s, x as f32 * s, s, y as f32 * s], used: frame });
                    return Some(self.slots.len() - 1);
                }
            }
        }
        None
    }

    /// Frees the least recently used tile not used this frame whose size
    /// is this tier's or larger, trying the smaller sizes after.
    fn evict(&mut self, tier: usize, frame: u64) -> bool {
        for t in tier..4 {
            let pick = self.slots.iter().enumerate().filter(|(_, s)| s.used != frame && s.tier <= t).min_by_key(|(_, s)| s.used).map(|(i, _)| i);
            if let Some(i) = pick {
                let s = self.slots.swap_remove(i);
                self.layers[s.layer as usize] &= !s.mask;
                return true;
            }
        }
        false
    }

    /// The light's tile, made if it has none (`Some((slot, fresh))`).
    pub fn take(&mut self, key: u32, tier: usize, frame: u64) -> Option<(u32, [f32; 4], bool)> {
        if let Some(s) = self.slots.iter_mut().find(|s| s.key == key && s.tier == tier) {
            s.used = frame;
            return Some((s.layer, s.params, false));
        }
        let i = match self.alloc(key, tier, frame) {
            Some(i) => i,
            None if self.evict(tier, frame) => self.alloc(key, tier, frame)?,
            None => return None,
        };
        let s = &self.slots[i];
        Some((s.layer, s.params, true))
    }

    /// Forgets every tile (the layer was remade).
    pub fn clear(&mut self) {
        self.slots.clear();
        self.layers.iter_mut().for_each(|l| *l = 0);
    }
}

/// A tile to draw this frame: where, with what matrix (world to the
/// tile's clip), for which light.
#[derive(Clone, Debug)]
pub struct Task {
    pub layer: u32,
    pub rect: [u32; 4],
    pub matrix: Mat4,
    pub light: u32,
}

/// What a frame's packer asks for a shadowed light.
pub struct Candidate<'a> {
    /// The light's place in the packed list.
    pub pos: usize,
    /// The light's index among the map's lights (the tile's key).
    pub key: u32,
    pub record: &'a [u8],
    pub priority: f32,
    pub dist2: f32,
    /// The view's box clipped to the light's (what it was ranked over).
    pub view_box: (Vec3, Vec3),
}

/// The per-frame choice and the tiles kept between frames.
pub struct Plan {
    pub layer_size: u32,
    atlas: Atlas,
    frame: u64,
    /// The fit and matrix each light's tile was drawn with.
    drawn: std::collections::HashMap<u32, (Fit, Mat4)>,
    pub tasks: Vec<Task>,
    /// Lights shadowed last frame (for the profile line).
    pub last_count: usize,
    /// The packing or the tiles' drawing panicked: no light is shadowed
    /// for the rest of the map (lit as with `BO2MP_T7_SPOT_SHADOWS=0`)
    /// instead of the game ending; the panic hook logged where.
    pub failed: bool,
}

impl Plan {
    pub fn new(layer_size: u32, layers: u32) -> Self {
        Self { layer_size, atlas: Atlas::new(layers), frame: 0, drawn: Default::default(), tasks: Vec::new(), last_count: 0, failed: false }
    }

    /// [`Self::choose_tiles`], failing soft: after a panic nothing is
    /// shadowed again on this map ([`Self::failed`]).
    pub fn choose(&mut self, cands: Vec<Candidate>, max: usize, eye: Vec3) -> Vec<(usize, [u32; 60])> {
        if self.failed {
            self.tasks.clear();
            self.last_count = 0;
            return Vec::new();
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.choose_tiles(cands, max, eye))) {
            Ok(out) => out,
            Err(_) => {
                self.fail();
                Vec::new()
            }
        }
    }

    /// Stops the shadows for the map (a panic in their code).
    pub fn fail(&mut self) {
        self.failed = true;
        self.tasks.clear();
        self.last_count = 0;
        self.forget();
    }

    /// Ranks the candidates and gives the first `max` a tile: `(place in
    /// the list, shadow record)` per light given one, in rank order.
    pub fn choose_tiles(&mut self, mut cands: Vec<Candidate>, max: usize, eye: Vec3) -> Vec<(usize, [u32; 60])> {
        self.frame += 1;
        self.tasks.clear();
        cands.retain(|c| c.priority > 0.0);
        cands.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal).then(a.dist2.partial_cmp(&b.dist2).unwrap_or(std::cmp::Ordering::Equal)));
        let mut out = Vec::new();
        for c in cands {
            if out.len() >= max.min(MAX_RECORDS) {
                break;
            }
            let r = c.record;
            let omni = u(r, 0x40) == 4;
            let (lo, hi) = light_box(r);
            let m = if omni {
                // (Looking at the centre of the box the ranking used.)
                omni_matrix(r, (c.view_box.0 + c.view_box.1) * 0.5)
            } else {
                light_matrix(r)
            };
            let Some(fit) = fit(&m, &corners(lo, hi), true) else { continue };
            let tier = if omni { OMNI_TIER } else { SPOT_TIER };
            let Some((layer, tile, fresh)) = self.atlas.take(c.key, tier, self.frame) else { continue };
            let redraw = fresh || self.drawn.get(&c.key).is_none_or(|(f0, m0)| *f0 != fit || !m0.abs_diff_eq(m, 1e-4));
            if redraw {
                let s = self.layer_size as f32;
                let rect = [(tile[1] * s).round() as u32, (tile[3] * s).round() as u32, (tile[0] * s).round() as u32, (tile[2] * s).round() as u32];
                self.tasks.push(Task { layer, rect, matrix: fit.crop() * m, light: c.key });
                self.drawn.insert(c.key, (fit, m));
            }
            let radius = if omni { 1.0 } else { f(r, 0x94) } / self.layer_size as f32;
            let rel = m * Mat4::from_translation(eye);
            out.push((c.pos, record(&rel, layer, tile, &fit, radius)));
        }
        self.last_count = out.len();
        out
    }

    /// A tile that was not drawn: drawn the next time its light is chosen.
    pub fn undrawn(&mut self, light: u32) {
        self.drawn.remove(&light);
    }

    /// The tiles were lost (the layer was remade): draw them again.
    pub fn forget(&mut self) {
        self.atlas.clear();
        self.drawn.clear();
    }
}

/// The box of the view (its frustum out to `far`) clipped to `lo..hi`.
pub fn view_box(inv_view_projection: &Mat4, eye: Vec3, near: f32, far: f32, lo: Vec3, hi: Vec3) -> Option<(Vec3, Vec3)> {
    let (mut a, mut b) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for d in [near, far.max(near * 2.0)] {
        // (Reversed infinite depth: z = near / distance.)
        let z = near / d;
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let p = *inv_view_projection * Vec4::new(x, y, z, 1.0);
            let p = p.truncate() / p.w + eye;
            a = a.min(p);
            b = b.max(p);
        }
    }
    let (a, b) = (a.max(lo), b.min(hi));
    (a.x <= b.x && a.y <= b.y && a.z <= b.z).then_some((a, b))
}

const WGSL: &str = r#"
struct U { m: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> inst: array<u32>;
@group(0) @binding(2) var<storage, read> bones: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> ids: array<u32>;

@vertex fn vs_world(@location(0) p: vec3<f32>) -> @builtin(position) vec4<f32> {
    return u.m * vec4<f32>(p, 1.0);
}

@vertex fn vs_model(@location(0) p: vec3<f32>, @builtin(instance_index) i: u32) -> @builtin(position) vec4<f32> {
    let b = inst[ids[i] * 6u + 3u] * 4u;
    let q = vec4<f32>(p, 1.0);
    let w = vec3<f32>(dot(bones[b], q), dot(bones[b + 1u], q), dot(bones[b + 2u], q));
    return u.m * vec4<f32>(w, 1.0);
}

@vertex fn vs_clear(@builtin(vertex_index) v: u32) -> @builtin(position) vec4<f32> {
    let x = f32((v << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(v & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}
"#;

/// The atlas's format (16-bit, as the exe's shadow maps).
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;

/// The casters' depth bias: the exe's shadow raster state (bias mode 3,
/// `crate::state`: -4 units, slope -4, clamp -0.1; away from the light in
/// reversed depth).
fn caster_bias() -> wgpu::DepthBiasState {
    wgpu::DepthBiasState { constant: -4, slope_scale: -4.0, clamp: -0.1 }
}

const UNIFORM_STRIDE: u64 = 256;
/// Tiles drawn in one frame at most (the rest the next frames).
pub const TASKS_PER_FRAME: usize = 8;

/// The atlas on the GPU and its depth-only caster passes.
pub struct Tiles {
    pub texture: wgpu::Texture,
    /// The whole array, as the lighting binds it.
    pub view: wgpu::TextureView,
    layer_views: Vec<wgpu::TextureView>,
    world: wgpu::RenderPipeline,
    model: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    ids: wgpu::Buffer,
    ids_cap: usize,
    group: Option<wgpu::BindGroup>,
    /// Scratch kept between frames.
    scratch_ids: Vec<u32>,
    draws: Vec<(bool, u32, u32, i32, u32, u32)>,
}

impl Tiles {
    pub fn new(gpu: &render_h1::gpu::Gpu, size: u32, layers: u32, instances: usize) -> Self {
        let device = &gpu.device;
        let texture = crate::audit::texture(gpu, &wgpu::TextureDescriptor {
            label: Some("gSpotShadowmapArray"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let layer_views = (0..layers).map(|l| texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: l, array_layer_count: Some(1), ..Default::default() })).collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 spot shadow casters"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let storage = |b: u32| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("t7 spot shadow casters"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(64) }, count: None },
                storage(1),
                storage(2),
                storage(3),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("t7 spot shadow casters"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let attrs = [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }];
        let vb = [wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }];
        let pipe = |entry: &str, buffers: &[wgpu::VertexBufferLayout], compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(true), depth_compare: Some(compare), stencil: Default::default(), bias: if compare == wgpu::CompareFunction::Greater { caster_bias() } else { Default::default() } }),
                multisample: Default::default(),
                fragment: None,
                multiview_mask: None,
                cache: crate::pipecache::get(&device).as_ref(),
            })
        };
        let world = pipe("vs_world", &vb, wgpu::CompareFunction::Greater);
        let model = pipe("vs_model", &vb, wgpu::CompareFunction::Greater);
        let clear = pipe("vs_clear", &[], wgpu::CompareFunction::Always);
        let uniforms = crate::audit::buffer(device, &wgpu::BufferDescriptor { label: Some("t7 spot shadow matrices"), size: UNIFORM_STRIDE * TASKS_PER_FRAME as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let ids_cap = (instances.max(1) * 2).max(1024);
        let ids = crate::audit::buffer(device, &wgpu::BufferDescriptor { label: Some("t7 spot shadow caster ids"), size: 4 * ids_cap as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { texture, view, layer_views, world, model, clear, layout, uniforms, ids, ids_cap, group: None, scratch_ids: Vec::new(), draws: Vec::new() }
    }

    /// Draws this frame's tiles: per tile, its rectangle cleared to the far
    /// depth and the casters in its light's frustum, all in one pass.
    /// Returns the tiles drawn (the rest are put back for the next frame).
    pub fn record(&mut self, gpu: &render_h1::gpu::Gpu, enc: &mut wgpu::CommandEncoder, g: &crate::gbuffer::GBuffer, casters: &[usize], tasks: &mut Vec<Task>) -> usize {
        if tasks.is_empty() {
            return 0;
        }
        let sc = &g.scene;
        if self.group.is_none() {
            self.group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("t7 spot shadow casters"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.uniforms, offset: 0, size: wgpu::BufferSize::new(64) }) },
                    wgpu::BindGroupEntry { binding: 1, resource: g.instances.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: g.bones.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.ids.as_entire_binding() },
                ],
            }));
        }
        let n = tasks.len().min(TASKS_PER_FRAME);
        let mut mats = vec![0u8; UNIFORM_STRIDE as usize * n];
        self.scratch_ids.clear();
        self.draws.clear();
        let mut per_task = Vec::with_capacity(n);
        for (t, task) in tasks.iter().take(n).enumerate() {
            mats[t * UNIFORM_STRIDE as usize..t * UNIFORM_STRIDE as usize + 64].copy_from_slice(bytemuck::cast_slice(&task.matrix.to_cols_array()));
            let planes = clip_planes(&task.matrix);
            let first = self.draws.len();
            for &di in casters {
                let d = &sc.draws[di];
                if d.group == 3 {
                    if box_in(&planes, d.mins, d.maxs) {
                        self.draws.push((false, d.first_index, d.index_count, d.base_vertex, 0, 1));
                    }
                    continue;
                }
                let start = self.scratch_ids.len();
                match sc.cull_ranges.get(d.cull as usize) {
                    Some(range) => {
                        for k in 0..d.instances {
                            let id = d.instance + k;
                            let s = range.spheres.get((id - range.first) as usize).copied().unwrap_or([0.0, 0.0, 0.0, -1.0]);
                            if s[3] < 0.0 || sphere_in(&planes, s) {
                                self.scratch_ids.push(id);
                            }
                        }
                    }
                    None => self.scratch_ids.extend(d.instance..d.instance + d.instances),
                }
                let count = (self.scratch_ids.len() - start) as u32;
                if count > 0 {
                    self.draws.push((true, d.first_index, d.index_count, d.base_vertex, start as u32, count));
                }
            }
            per_task.push(first..self.draws.len());
        }
        if self.scratch_ids.len() > self.ids_cap {
            // (More than the buffer holds: the instanced casters of the
            // tiles past it are left out this frame.)
            let cap = self.ids_cap as u32;
            self.draws.retain(|d| !d.0 || d.4 + d.5 <= cap);
            self.scratch_ids.truncate(self.ids_cap);
        }
        gpu.queue.write_buffer_p(&self.uniforms, 0, &mats);
        if !self.scratch_ids.is_empty() {
            gpu.queue.write_buffer_p(&self.ids, 0, bytemuck::cast_slice(&self.scratch_ids));
        }
        let group = self.group.as_ref().unwrap();
        // (Tiles of one layer in one pass; the layer's other tiles kept.)
        let mut layers: Vec<u32> = tasks.iter().take(n).map(|t| t.layer).collect();
        layers.sort_unstable();
        layers.dedup();
        for layer in layers {
            let mut pass = enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: Some("t7 spot shadow tiles"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.layer_views[layer as usize], depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for (t, task) in tasks.iter().take(n).enumerate().filter(|(_, t)| t.layer == layer) {
                let [x, y, w, h] = task.rect;
                pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, w, h);
                pass.set_bind_group_p(0, group, &[(t as u64 * UNIFORM_STRIDE) as u32]);
                pass.set_pipeline_p(&self.clear);
                pass.draw_p(0..3, 0..1);
                let range = per_task[t].clone();
                crate::timing::count_shadow_draws(range.len() as u64);
                if self.draws[range.clone()].iter().any(|d| !d.0) {
                    pass.set_pipeline_p(&self.world);
                    pass.set_vertex_buffer(0, g.world[0].slice(..));
                    pass.set_index_buffer(g.world[2].slice(..), wgpu::IndexFormat::Uint16);
                    for d in self.draws[range.clone()].iter().filter(|d| !d.0) {
                        pass.draw_indexed_p(d.1..d.1 + d.2, d.3, 0..1);
                    }
                }
                if self.draws[range.clone()].iter().any(|d| d.0) {
                    pass.set_pipeline_p(&self.model);
                    pass.set_vertex_buffer(0, g.models[0].slice(..));
                    pass.set_index_buffer(g.models[3].slice(..), wgpu::IndexFormat::Uint16);
                    for d in self.draws[range].iter().filter(|d| d.0) {
                        pass.draw_indexed_p(d.1..d.1 + d.2, d.3, d.4..d.4 + d.5);
                    }
                }
            }
        }
        tasks.drain(..n);
        n
    }
}

/// The stage's spot shadows: the plan kept between frames and the atlas.
pub struct SpotShadows {
    pub plan: Plan,
    pub tiles: Tiles,
    /// The G-buffer draws that cast (`GBuffer::shadow_casters`), taken at
    /// the first tile drawn.
    casters: Option<Vec<usize>>,
    /// Tiles drawn in all (the report).
    pub drawn: u64,
    /// The failure line was written (`Plan::failed`).
    pub failure_said: bool,
}

impl SpotShadows {
    pub fn new(gpu: &render_h1::gpu::Gpu, layer_size: u32, instances: usize) -> Self {
        Self { plan: Plan::new(layer_size, 1), tiles: Tiles::new(gpu, layer_size, 1, instances), casters: None, drawn: 0, failure_said: false }
    }

    /// Draws the tiles the plan asked for this frame.
    pub fn record(&mut self, gpu: &render_h1::gpu::Gpu, enc: &mut wgpu::CommandEncoder, g: &crate::gbuffer::GBuffer) -> usize {
        if self.plan.tasks.is_empty() {
            return 0;
        }
        let (casters, tiles, plan) = (&mut self.casters, &mut self.tiles, &mut self.plan);
        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let casters = casters.get_or_insert_with(|| g.shadow_casters());
            tiles.record(gpu, enc, g, casters, &mut plan.tasks)
        }));
        let Ok(n) = drawn else {
            // (A pass left open by the panic was dropped with it; the
            // frame's encoder goes on.)
            self.plan.fail();
            return 0;
        };
        // (Tiles past this frame's share: drawn the next frames, their
        // lights left unshadowed until then.)
        for t in std::mem::take(&mut self.plan.tasks) {
            self.plan.undrawn(t.light);
        }
        self.drawn += n as u64;
        n
    }
}

/// The clip volume's planes (inward) of a world-to-clip matrix (depth 0..1).
pub fn clip_planes(m: &Mat4) -> [Vec4; 6] {
    let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
    [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| p / p.truncate().length().max(1e-12))
}

fn box_in(planes: &[Vec4; 6], lo: [f32; 3], hi: [f32; 3]) -> bool {
    planes.iter().all(|p| {
        let x = if p.x >= 0.0 { hi[0] } else { lo[0] };
        let y = if p.y >= 0.0 { hi[1] } else { lo[1] };
        let z = if p.z >= 0.0 { hi[2] } else { lo[2] };
        p.x * x + p.y * y + p.z * z + p.w >= 0.0
    })
}

fn sphere_in(planes: &[Vec4; 6], s: [f32; 4]) -> bool {
    planes.iter().all(|p| p.x * s[0] + p.y * s[1] + p.z * s[2] + p.w >= -s[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morton_cells() {
        assert_eq!((0..4).map(morton).collect::<Vec<_>>(), vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
        assert_eq!(morton(63), (7, 7));
        assert_eq!(morton(6), (2, 1));
    }

    #[test]
    fn tiles_take_their_cells_and_the_oldest_unused_goes() {
        let mut a = Atlas::new(1);
        // 16 omni tiles fill the layer (4 cells each).
        for k in 0..16 {
            let (layer, p, fresh) = a.take(k, OMNI_TIER, 1).unwrap();
            assert!(fresh && layer == 0);
            let (x, y) = morton(k);
            assert_eq!(p, [0.25, x as f32 * 0.25, 0.25, y as f32 * 0.25]);
        }
        // Taken again: the same tile, not fresh.
        assert_eq!(a.take(3, OMNI_TIER, 2).map(|t| t.2), Some(false));
        // Full; a spot tile evicts the oldest tile unused this frame.
        let (_, p, fresh) = a.take(100, SPOT_TIER, 2).unwrap();
        assert!(fresh);
        assert_eq!(p[0], 0.125);
        assert!(a.slots.iter().all(|s| s.key != 0));
        // Every tile used this frame: nothing to evict.
        for k in 1..16 {
            let _ = a.take(k, OMNI_TIER, 3);
        }
        let _ = a.take(100, SPOT_TIER, 3);
        assert!(a.take(200, OMNI_TIER, 3).is_none());
    }

    #[test]
    fn the_fit_maps_the_box_to_the_tile_and_its_depth_to_0_1() {
        let m = spot_matrix(Vec3::new(0.0, 0.0, 500.0), Vec3::NEG_Z, 0.5f32.sqrt(), 10.0, 1000.0);
        let (lo, hi) = (Vec3::new(-100.0, -50.0, 0.0), Vec3::new(100.0, 50.0, 100.0));
        let fit = fit(&m, &corners(lo, hi), true).unwrap();
        let c = fit.crop() * m;
        for p in corners(lo, hi) {
            let q = c * p.extend(1.0);
            let (x, y, z) = (q.x / q.w, q.y / q.w, q.z / q.w);
            assert!((-1.0001..=1.0001).contains(&x) && (-1.0001..=1.0001).contains(&y), "{x} {y}");
            assert!((-0.0001..=1.0001).contains(&z), "{z}");
        }
        // The box's corners reach the tile's edges and the depth range.
        let zs: Vec<f32> = corners(lo, hi).iter().map(|p| {
            let q = c * p.extend(1.0);
            q.z / q.w
        }).collect();
        assert!(zs.iter().any(|z| (z - 1.0).abs() < 1e-3) && zs.iter().any(|z| z.abs() < 1e-3));
        // The lighting's lookup of a point (the record's words) lands at
        // the same tile uv and depth the casters' matrix gives it.
        let p = Vec3::new(30.0, -20.0, 40.0);
        let q = m * p.extend(1.0);
        let uv = Vec3::new(q.x / q.w * 0.5 + 0.5, 0.5 - q.y / q.w * 0.5, 0.0);
        let tu = (uv.x - fit.offset[0]) * fit.scale[0];
        let tv = (uv.y - fit.offset[1]) * fit.scale[1];
        let d = (q.z + -fit.depth[0] * q.w) / (q.w * (fit.depth[1] - fit.depth[0]));
        let k = c * p.extend(1.0);
        assert!((tu - (k.x / k.w * 0.5 + 0.5)).abs() < 1e-4);
        assert!((tv - (0.5 - k.y / k.w * 0.5)).abs() < 1e-4);
        assert!((d - k.z / k.w).abs() < 1e-4);
    }

    /// After a failure the plan shadows nothing more on the map (the lights
    /// are packed unshadowed) and keeps no tile to draw.
    #[test]
    fn a_failed_plan_shadows_nothing_more() {
        let mut p = Plan::new(2048, 1);
        assert!(p.choose(Vec::new(), 4, Vec3::ZERO).is_empty());
        p.tasks.push(Task { layer: 0, rect: [0, 0, 256, 256], matrix: Mat4::IDENTITY, light: 1 });
        p.fail();
        assert!(p.failed && p.tasks.is_empty());
        let mut r = vec![0u8; 0x238];
        r[0x40..0x44].copy_from_slice(&2u32.to_le_bytes());
        let c = Candidate { pos: 0, key: 1, record: &r, priority: 1.0, dist2: 1.0, view_box: (Vec3::ZERO, Vec3::ONE) };
        assert!(p.choose(vec![c], 4, Vec3::ZERO).is_empty());
        assert!(p.tasks.is_empty() && p.last_count == 0);
    }

    #[test]
    fn reversed_depth_spot() {
        let m = spot_matrix(Vec3::ZERO, Vec3::X, 0.5, 10.0, 100.0);
        let z = |x: f32| {
            let q = m * Vec4::new(x, 0.0, 0.0, 1.0);
            q.z / q.w
        };
        assert!((z(10.0) - 1.0).abs() < 1e-5 && z(100.0).abs() < 1e-5 && z(50.0) < z(20.0));
    }

    #[test]
    fn the_record_holds_the_eye_relative_projection() {
        let m = spot_matrix(Vec3::new(10.0, 20.0, 30.0), Vec3::Y, 0.7, 5.0, 300.0);
        let eye = Vec3::new(-50.0, 4.0, 9.0);
        let fit = Fit { scale: [1.0, 1.0], offset: [0.0, 0.0], depth: [0.0, 1.0] };
        let r = record(&(m * Mat4::from_translation(eye)), 3, [0.125, 0.25, 0.125, 0.5], &fit, 0.001);
        let p = Vec3::new(12.0, 90.0, 31.0);
        let q = m * p.extend(1.0);
        let rel = (p - eye).extend(1.0);
        let dot = |w: usize| (0..4).map(|i| f32::from_bits(r[w + i]) * rel[i]).sum::<f32>();
        assert!((dot(17) - q.x).abs() < 1e-2 && (dot(21) - q.y).abs() < 1e-2 && (dot(25) - q.z).abs() < 1e-3 && (dot(29) - q.w).abs() < 1e-2);
        assert_eq!(r[7], 3);
        assert_eq!(f32::from_bits(r[34]), 0.25);
    }
}
