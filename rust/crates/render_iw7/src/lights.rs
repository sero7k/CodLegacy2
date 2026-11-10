//! IW7's clustered local lights, as the lit pixel programs read them
//! (`techsets_mp_riot`'s `lmap_*` / `lprobe_*` programs):
//! - the pixel's tile is its position over 8 (tiles across: `(width + 7) /
//!   8`, the width code 109's x) and its depth slice its view depth over 32
//!   (at most 8191);
//! - t37 (one word per slice): the slice's first light index in the high
//!   16 bits, its last in the low 16;
//! - t35 (`words` words per tile, code 147's x as an integer): a bit per
//!   light, light `32 w + b` in word `w` bit `b`;
//! - t24 (88 bytes per light): colour (0), the spot's axis toward the light
//!   and its shadow index in w's low byte (16; 255 none), flags (28.x, bits
//!   8-9 the kind, 2 a spot) and the bulb's half segment (28.yzw), the
//!   bulb's radius (44: the light falls off as `(radius / distance)^2`),
//!   position about the eye and one over the reach (48), the cone's scale and bias on
//!   the cosine (64: `saturate(cos * scale + bias)`), the reach's falloff
//!   scale and the angular profile's lookup (76: x, then y, z into the 1D
//!   code texture 4);
//! - t25: spot shadows (`spot`).
//!
//! The lights are the map's primary lights of kind 2 (spot) and 3 (omni),
//! sorted by depth each frame so each slice's lights lie close together;
//! a slice's range covers every light whose reach meets it, and a tile's
//! bits every light whose screen rectangle meets it.
//! `BO2MP_IW7_LOCAL_LIGHTS=0`: none.

use fastfile_iw7::map::Iw7Map;
use glam::{Mat4, Vec3, Vec4};
use render_h1::gpu::Gpu;

/// The most lights a frame takes (32 a word).
pub const MAX_LIGHTS: usize = 256;
pub const MAX_WORDS: usize = MAX_LIGHTS / 32;
/// The tiles of a 4K frame.
pub const MAX_TILES: usize = 480 * 270;
pub const SLICES: usize = 8192;
pub const SLICE_DEPTH: f32 = 32.0;
const RECORD_WORDS: usize = 22;

/// One local light.
#[derive(Clone, Copy, Debug)]
pub struct LocalLight {
    pub pos: Vec3,
    pub reach: f32,
    pub colour: [f32; 3],
    /// Toward the light from what it lights (the spot's axis).
    pub toward: Vec3,
    pub spot: bool,
    pub cos_outer: f32,
    pub cos_inner: f32,
    pub up: Vec3,
    pub bulb_radius: f32,
    pub bulb_length: f32,
    /// Whether the map lets it cast shadows (`canUseShadowMap`).
    pub casts: bool,
    /// Its spot shadow's index (`spot`).
    pub shadow: Option<u8>,
}

impl LocalLight {
    /// The light's record (22 words; the vec4s at bytes 16 and 28 share
    /// word 7: the shadow index in its low byte, the kind in bits 8-9).
    /// Its position is about the eye, as the programs' pixel positions are
    /// (the vertex programs subtract the eye).
    pub fn record(&self, eye: Vec3) -> [u32; RECORD_WORDS] {
        self.record_with(eye, true)
    }

    /// The record, its shadow left out unless `shadow`.
    pub fn record_with(&self, eye: Vec3, shadow: bool) -> [u32; RECORD_WORDS] {
        let mut r = [0u32; RECORD_WORDS];
        let mut f = |i: usize, v: f32| r[i] = v.to_bits();
        f(0, self.colour[0]);
        f(1, self.colour[1]);
        f(2, self.colour[2]);
        let t = self.toward.normalize_or(Vec3::Z);
        f(4, t.x);
        f(5, t.y);
        f(6, t.z);
        // (A point bulb keeps a sliver of a segment: the programs divide by
        // its square length.)
        let half = (self.bulb_length * 0.5).max(1e-3);
        let seg = self.up.normalize_or(Vec3::X) * half;
        f(8, seg.x);
        f(9, seg.y);
        f(10, seg.z);
        f(11, self.bulb_radius.max(0.01));
        let rel = self.pos - eye;
        f(12, rel.x);
        f(13, rel.y);
        f(14, rel.z);
        f(15, 1.0 / self.reach.max(1.0));
        // The cone: 0 at the outer cosine, 1 at the inner.
        let span = (self.cos_inner - self.cos_outer).max(1e-3);
        f(16, 1.0 / span);
        f(17, -self.cos_outer / span);
        f(19, 1.0);
        // (The angular profile's lookup: texel 0 of a white texture.)
        f(20, 0.0);
        f(21, 0.0);
        r[7] = u32::from(self.shadow.filter(|_| shadow).unwrap_or(255)) | if self.spot { 2 << 8 } else { 3 << 8 };
        r
    }
}

/// The map's local lights (`BO2MP_IW7_LOCAL_LIGHTS=0`: none).
pub fn of_map(map: &Iw7Map) -> Vec<LocalLight> {
    if std::env::var("BO2MP_IW7_LOCAL_LIGHTS").is_ok_and(|v| v.trim() == "0") {
        return Vec::new();
    }
    map.lights
        .iter()
        .filter(|l| matches!(l.kind, 2 | 3) && l.radius > 0.0 && l.color.iter().any(|&c| c > 0.0))
        .map(|l| LocalLight {
            pos: Vec3::from(l.origin),
            reach: l.radius,
            colour: l.color,
            toward: Vec3::from(l.dir),
            spot: l.kind == 2,
            cos_outer: l.cos_outer,
            cos_inner: l.cos_inner.max(l.cos_outer + 1e-3),
            up: Vec3::from(l.up),
            bulb_radius: l.bulb_radius,
            bulb_length: l.bulb_length.max(0.0),
            casts: l.flags[0] != 0,
            shadow: None,
        })
        .collect()
}

/// The cluster buffers.
pub struct Clusters {
    pub lights: Vec<LocalLight>,
    pub records: wgpu::Buffer,
    pub shadows: wgpu::Buffer,
    pub tiles: wgpu::Buffer,
    pub slices: wgpu::Buffer,
    /// Words per tile this frame (code 147's x).
    pub words: u32,
    tile_words: Vec<u32>,
    slice_words: Vec<u32>,
    seen: Vec<(f32, usize)>,
    debug: bool,
    /// The quality's switches: the lights' direct light, their shadows.
    pub enabled: bool,
    pub shadows_on: bool,
    records_scratch: Vec<u32>,
}

impl Clusters {
    pub fn new(g: &Gpu, lights: Vec<LocalLight>) -> Self {
        let buf = |label: &str, size: usize| g.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self {
            lights,
            records: buf("iw7 light records", MAX_LIGHTS * RECORD_WORDS * 4),
            shadows: buf("iw7 light shadows", 256 * 24 * 4),
            tiles: buf("iw7 light tiles", MAX_TILES * MAX_WORDS * 4),
            slices: buf("iw7 light slices", SLICES * 4),
            words: 1,
            tile_words: Vec::new(),
            slice_words: vec![0; SLICES],
            seen: Vec::with_capacity(MAX_LIGHTS),
            debug: std::env::var("IW7_DBG_LIGHTS").is_ok(),
            enabled: true,
            shadows_on: true,
            records_scratch: Vec::with_capacity(MAX_LIGHTS * RECORD_WORDS),
        }
    }

    /// Bins the lights for a frame: `view_projection` about `eye`, the
    /// view's forward axis, the frame's size.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, g: &Gpu, view_projection: Mat4, eye: Vec3, forward: Vec3, width: u32, height: u32, shadow_records: &[u32]) -> u64 {
        g.queue.write_buffer(&self.shadows, 0, bytemuck::cast_slice(shadow_records));
        let fwd = forward.normalize_or(Vec3::X);
        // The lights in front (or round the eye), nearest first.
        let mut seen = std::mem::take(&mut self.seen);
        seen.clear();
        seen.extend(self.lights.iter().enumerate().filter(|_| self.enabled).filter_map(|(i, l)| {
            let d = (l.pos - eye).dot(fwd);
            (d + l.reach > 0.0 && (l.pos - eye).length() < 20000.0 + l.reach).then_some((d, i))
        }));
        seen.sort_by(|a, b| a.0.total_cmp(&b.0));
        seen.truncate(MAX_LIGHTS);
        let n = seen.len();
        self.words = (n.div_ceil(32)).max(1) as u32;
        let words = self.words as usize;
        let tiles_x = (width as usize).div_ceil(8);
        let tiles_y = (height as usize).div_ceil(8);
        let tiles = (tiles_x * tiles_y).min(MAX_TILES / words.max(1));
        self.tile_words.clear();
        self.tile_words.resize(tiles * words, 0);
        // Empty slices: first 1, last 0 (no light).
        self.slice_words.fill(1 << 16);
        let mut records = std::mem::take(&mut self.records_scratch);
        records.clear();
        for (i, &(d, li)) in seen.iter().enumerate() {
            let l = &self.lights[li];
            records.extend_from_slice(&l.record_with(eye, self.shadows_on));
            // Its depth slices.
            let s0 = (((d - l.reach).max(0.0) / SLICE_DEPTH) as usize).min(SLICES - 1);
            let s1 = (((d + l.reach).max(0.0) / SLICE_DEPTH) as usize).min(SLICES - 1);
            for s in s0..=s1 {
                let w = self.slice_words[s];
                let (first, last) = (w >> 16, w & 0xffff);
                let (first, last) = if first > last { (i as u32, i as u32) } else { (first.min(i as u32), last.max(i as u32)) };
                self.slice_words[s] = (first << 16) | last;
            }
            // Its screen rectangle (the whole screen when the eye is inside).
            let rect = screen_rect(view_projection, l.pos - eye, l.reach, width, height);
            let (x0, y0, x1, y1) = match rect {
                Some((x0, y0, x1, y1)) => ((x0 / 8.0).floor().max(0.0) as usize, (y0 / 8.0).floor().max(0.0) as usize, ((x1 / 8.0).ceil() as usize).min(tiles_x), ((y1 / 8.0).ceil() as usize).min(tiles_y)),
                None => (0, 0, tiles_x, tiles_y),
            };
            let (wi, bit) = (i / 32, 1u32 << (i % 32));
            for ty in y0..y1 {
                for tx in x0..x1 {
                    let t = ty * tiles_x + tx;
                    if t < tiles {
                        self.tile_words[t * words + wi] |= bit;
                    }
                }
            }
        }
        if records.is_empty() {
            records.resize(RECORD_WORDS, 0);
        }
        if self.debug {
            let used: Vec<usize> = (0..SLICES).filter(|&s| self.slice_words[s] >> 16 <= (self.slice_words[s] & 0xffff)).collect();
            let lit_tiles = self.tile_words.iter().filter(|w| **w != 0).count();
            eprintln!("lights: {n} of {} seen, {} words, slices used {}..{:?}, tiles lit {lit_tiles} of {tiles}", self.lights.len(), self.words, used.first().copied().unwrap_or(0), used.last());
        }
        g.queue.write_buffer(&self.records, 0, bytemuck::cast_slice(&records));
        if !self.tile_words.is_empty() {
            g.queue.write_buffer(&self.tiles, 0, bytemuck::cast_slice(&self.tile_words));
        }
        g.queue.write_buffer(&self.slices, 0, bytemuck::cast_slice(&self.slice_words));
        let bytes = ((shadow_records.len() + records.len() + self.tile_words.len() + self.slice_words.len()) * 4) as u64;
        self.seen = seen;
        self.records_scratch = records;
        bytes
    }
}

/// A sphere's screen rectangle in pixels (`rel` about the eye), or `None`
/// when the eye is inside it or it crosses the eye's plane.
fn screen_rect(vp: Mat4, rel: Vec3, r: f32, width: u32, height: u32) -> Option<(f32, f32, f32, f32)> {
    if rel.length() <= r * 1.05 {
        return None;
    }
    let mut lo = [f32::MAX; 2];
    let mut hi = [f32::MIN; 2];
    for k in 0..8 {
        let c = rel + Vec3::new(if k & 1 == 0 { -r } else { r }, if k & 2 == 0 { -r } else { r }, if k & 4 == 0 { -r } else { r });
        let p = vp * Vec4::new(c.x, c.y, c.z, 1.0);
        if p.w <= 1e-3 {
            return None;
        }
        let x = (p.x / p.w * 0.5 + 0.5) * width as f32;
        let y = (0.5 - p.y / p.w * 0.5) * height as f32;
        lo = [lo[0].min(x), lo[1].min(y)];
        hi = [hi[0].max(x), hi[1].max(y)];
    }
    (hi[0] >= 0.0 && hi[1] >= 0.0 && lo[0] <= width as f32 && lo[1] <= height as f32).then_some((lo[0], lo[1], hi[0], hi[1])).or(Some((0.0, 0.0, 0.0, 0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_puts_each_field_where_the_programs_read_it() {
        let l = LocalLight { pos: Vec3::new(1.0, 2.0, 3.0), reach: 100.0, colour: [10.0, 20.0, 30.0], toward: Vec3::Z, spot: true, cos_outer: 0.5, cos_inner: 0.9, up: Vec3::Y, bulb_radius: 1.0, bulb_length: 0.0, casts: false, shadow: None };
        let r = l.record(Vec3::ZERO);
        assert_eq!(f32::from_bits(r[1]), 20.0);
        assert_eq!(f32::from_bits(r[6]), 1.0);
        assert_eq!(r[7] & 255, 255);
        assert_eq!((r[7] >> 8) & 3, 2);
        assert_eq!(f32::from_bits(r[13]), 2.0);
        assert_eq!(f32::from_bits(r[15]), 0.01);
        // The cone: 0 at the outer cosine, 1 at the inner.
        let cone = |c: f32| (c * f32::from_bits(r[16]) + f32::from_bits(r[17])).clamp(0.0, 1.0);
        assert!(cone(0.5).abs() < 1e-5 && (cone(0.9) - 1.0).abs() < 1e-4);
    }
}
