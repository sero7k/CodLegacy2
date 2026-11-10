//! A Modern Warfare III map's own light data, found in its zone: the
//! reflection probes (where each stands and its picture) and the images
//! the volume lighting is made of.
//!
//! None of the multiplayer maps read (Rust, Shipment, Afghan, Derail,
//! Estate, Highrise, Favela) has lightmaps on its world surfaces: every
//! surface record names lightmap 0 or 510 (none) and carries no lightmap
//! UVs. The world is lit by volumes and probes instead:
//!
//! - **Reflection probes** (`use 0x2b`, BC6H 256x256 with 6 levels, 9 to 47
//!   a map). Each is an octahedral map of the light seen from the probe
//!   (centre = up, the diamond's corners = down; linear HDR, the sky about
//!   1 to 2, no sun in it). Their pictures sit in the image run in asset
//!   order; the zone has an array of back-pointers to them (one 8-byte
//!   pointer per probe, in order) followed by one 52-byte record each:
//!   ```text
//!   +0x00 u8[16] guid
//!   +0x10 f32[3] origin (world units)
//!   +0x1c u32[3] 0
//!   +0x28 u32, +0x2c u32 (vary per map: not relied on)
//!   +0x30 u32 0xfff00000 | index (the anchor the records are found by)
//!   ```
//!   Some probes' pictures are in the zone whole (flags without a streamed
//!   group); the rest stream from the install's packages in two groups (the
//!   levels 128 down to 8, then the 256), which the install may not hold.
//! - **Volume images** (`use 0x21`, all in the zone): a BC6H 3-D volume (Rust
//!   48 x 48 x 80: ten bricks of 8 slices, the odd bricks about 0.5 and
//!   dense, the even ones sparse warm light), a 4-byte 3-D index volume
//!   (100 x 92 x 16), a 4-byte 2-D index (69 x 66), a 6-slice half-float
//!   grid (70 x 67), a BC4 coverage mask (552 x 528: white over the play
//!   area), and an 8-bit 2-D (556 x 532) and 3-D (556 x 532 x 8) pair. The
//!   `gfx_map` header holds their extents (the world box, texel sizes and
//!   scale/bias pairs) beside the pointers; [`read_grids`] reads the
//!   pointers and the extents it can recognise. How a shader weights them
//!   is not known here.
//! - Also `use 0x11`: a 4096 x 4096 16-bit top-down height map of the
//!   world (1024 x 512 on Shipment) with an 8-bit companion.

use super::index::{ZoneImage, f32_at, find_all, u32_at, u64_at};
use super::materials::{MapImage, Packages};
use crate::image::{self, ImageRec, ImageRun};
use crate::oodle::Decompressor;

/// An image's `use` byte for reflection probes.
pub const PROBE_USE: u8 = 0x2b;
/// DXGI BC6H (unsigned half-float).
const BC6H: u32 = 95;
/// A probe record's size.
pub const PROBE_RECORD: usize = 52;

/// A reflection probe: where it stands and its picture.
#[derive(Clone, Debug)]
pub struct ReflectionProbe {
    pub index: usize,
    pub origin: [f32; 3],
    pub image: MapImage,
}

/// A probe's picture decoded: an octahedral map of linear HDR light.
#[derive(Clone, Debug)]
pub struct ProbeMap {
    pub size: usize,
    pub rgb: Vec<[f32; 3]>,
}

/// The light a probe has toward the up, side and down directions: the
/// picture's radiance weighted by the cosine to the direction, over the
/// hemisphere (linear HDR).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProbeAmbient {
    pub up: [f32; 3],
    pub side: [f32; 3],
    pub down: [f32; 3],
}

impl ProbeAmbient {
    /// The ambient light for a surface with unit normal `n` (z up): the
    /// up, side and down values blended by the normal's height. (The
    /// octahedral map's horizontal turn is not known, so no direction
    /// about the vertical is used.)
    #[must_use]
    pub fn toward(&self, n: [f32; 3]) -> [f32; 3] {
        let (u, d) = (n[2].max(0.0), (-n[2]).max(0.0));
        let s = 1.0 - n[2].abs();
        std::array::from_fn(|k| self.up[k] * u + self.down[k] * d + self.side[k] * s)
    }
}

/// The unit direction of an octahedral map's point `(u, v)` in -1..1
/// (z up: the centre is up, the corners down).
#[must_use]
pub fn octahedral_direction(u: f32, v: f32) -> [f32; 3] {
    let z = 1.0 - u.abs() - v.abs();
    let (mut x, mut y) = (u, v);
    if z < 0.0 {
        x = (1.0 - v.abs()) * u.signum();
        y = (1.0 - u.abs()) * v.signum();
    }
    let l = (x * x + y * y + z * z).sqrt().max(1e-12);
    [x / l, y / l, z / l]
}

impl ProbeMap {
    /// The cosine-weighted radiance toward `axis`, weighting each texel by
    /// its solid angle (an octahedral texel's is about `1 / (|x|+|y|+|z|)^3`).
    fn lobe(&self, axis: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let (mut acc, mut wsum) = ([0.0f64; 3], 0.0f64);
        for y in 0..n {
            for x in 0..n {
                let d = octahedral_direction(
                    (x as f32 + 0.5) / n as f32 * 2.0 - 1.0,
                    (y as f32 + 0.5) / n as f32 * 2.0 - 1.0,
                );
                let c = d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2];
                if c <= 0.0 {
                    continue;
                }
                let solid = 1.0 / f64::from(d[0].abs() + d[1].abs() + d[2].abs()).powi(3);
                let w = solid * f64::from(c);
                let p = self.rgb[y * n + x];
                for k in 0..3 {
                    acc[k] += f64::from(p[k]) * w;
                }
                wsum += w;
            }
        }
        if wsum <= 0.0 { [0.0; 3] } else { std::array::from_fn(|k| (acc[k] / wsum) as f32) }
    }

    /// The up, side and down light; the side value averages the four
    /// horizontal axes.
    #[must_use]
    pub fn ambient(&self) -> ProbeAmbient {
        let up = self.lobe([0.0, 0.0, 1.0]);
        let down = self.lobe([0.0, 0.0, -1.0]);
        let sides = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]].map(|a| self.lobe(a));
        ProbeAmbient {
            up,
            down,
            side: std::array::from_fn(|k| sides.iter().map(|s| s[k]).sum::<f32>() / 4.0),
        }
    }
}

fn f16(b: u16) -> f32 {
    let e = i32::from((b >> 10) & 31);
    let m = f32::from(b & 1023);
    let v = match e {
        0 => m / 1024.0 * 2f32.powi(-14),
        31 => 65504.0,
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    };
    if b & 0x8000 != 0 { -v } else { v }
}

/// Decodes `w` x `h` BC6H blocks (unsigned) to linear RGB.
#[must_use]
pub fn decode_bc6h(data: &[u8], w: usize, h: usize) -> Option<Vec<[f32; 3]>> {
    if w % 4 != 0 || h % 4 != 0 || data.len() < w * h {
        return None;
    }
    let bw = w / 4;
    let mut out = vec![[0.0f32; 3]; w * h];
    let mut tmp = [0u16; 48];
    for by in 0..h / 4 {
        for bx in 0..bw {
            let o = (by * bw + bx) * 16;
            bcdec_rs::bc6h_half(data.get(o..o + 16)?, &mut tmp, 12, false);
            for y in 0..4 {
                for x in 0..4 {
                    let p = &mut out[(by * 4 + y) * w + bx * 4 + x];
                    for c in 0..3 {
                        p[c] = f16(tmp[y * 12 + x * 3 + c]).max(0.0);
                    }
                }
            }
        }
    }
    Some(out)
}

impl ReflectionProbe {
    /// The picture's largest level at most `max_dim` square that can be
    /// had (from the zone, else from the packages' groups), decoded.
    pub fn load(&self, paks: Option<&Packages>, max_dim: u32, oodle: &dyn Decompressor) -> Option<ProbeMap> {
        let img = &self.image;
        let (w0, h0) = (u32::from(img.width), u32::from(img.height));
        if w0 != h0 {
            return None;
        }
        let from_zone = |px: &[u8]| -> Option<(u32, Vec<u8>)> {
            let (mut w, mut off) = (w0, 0usize);
            while w > max_dim && w > 4 {
                off += image::level_bytes(BC6H, w, w)?;
                w /= 2;
            }
            let n = image::level_bytes(BC6H, w, w)?;
            px.get(off..off + n).map(|d| (w, d.to_vec()))
        };
        let size_and_bytes = if let Some(px) = &img.inline {
            from_zone(px)
        } else {
            let paks = paks?;
            let rec = ImageRec {
                at: 0,
                hash: img.hash,
                flags: 0,
                data_size: 0,
                width: img.width,
                height: img.height,
                depth: 1,
                array: 1,
                format: img.format,
                usage: PROBE_USE,
                levels: 0,
                groups: img.groups.clone(),
                inline: None,
                end: 0,
            };
            (0..img.groups.len()).rev().find_map(|g| {
                let levels = image::group_levels(&rec, g, BC6H)?;
                let &(_, w, _, off) = levels.iter().find(|l| l.1 <= max_dim)?;
                let bytes = paks.fetch(img.groups[g].key, oodle)?;
                let n = image::level_bytes(BC6H, w, w)?;
                bytes.get(off..off + n).map(|d| (w, d.to_vec()))
            })
        };
        let (w, bytes) = size_and_bytes?;
        Some(ProbeMap { size: w as usize, rgb: decode_bc6h(&bytes, w as usize, w as usize)? })
    }
}

/// An image of the volume lighting, whole (they are all in the zone).
#[derive(Clone, Debug, Default)]
pub struct VolumeImage {
    pub hash: u64,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub array: u16,
    pub format: u8,
    pub usage: u8,
    pub data: Vec<u8>,
}

/// What the volume lighting is made of (see the module notes). Fields are
/// `None` where the zone has no such image or the pointers did not line up.
#[derive(Clone, Debug, Default)]
pub struct Grids {
    /// BC6H 3-D volume (Rust 48 x 48 x 80).
    pub probe_volume: Option<VolumeImage>,
    /// 4-byte 2-D grid beside it (Rust 69 x 66) and 4-byte 3-D index
    /// volume (Rust 100 x 92 x 16).
    pub grid_2d: Option<VolumeImage>,
    pub index_3d: Option<VolumeImage>,
    /// Six half-float RGBA slices (Rust 70 x 67).
    pub six_way: Option<VolumeImage>,
    /// BC4 coverage mask, 8-bit 2-D and 8-bit 3-D (Rust 552 x 528, 556 x
    /// 532, 556 x 532 x 8).
    pub mask: Option<VolumeImage>,
    pub bytes_2d: Option<VolumeImage>,
    pub bytes_3d: Option<VolumeImage>,
    /// The world box the outdoor grids cover and its texel counts
    /// (mask width and height, then the coarse grid's), where the header
    /// block was recognised.
    pub box_min: Option<[f32; 3]>,
    pub box_max: Option<[f32; 3]>,
    pub dims: Option<[u32; 6]>,
    /// Where the header block with the outdoor pointers stands in the zone.
    pub block_at: Option<usize>,
}

/// A brick of the indoor probe volume: texels on a side (8 cells and one
/// shared border texel), 16 layers deep.
pub const BRICK: usize = 9;

/// The sparse table that says where each cell of the map's 2-D grid has
/// its brick in the probe volume's atlas (a map's `grid_2d` image: a u32 a
/// cell, `0xffffffff` empty). The high half of a cell is the brick's place
/// in the atlas, `column | row << 8` with 16 columns of [`BRICK`] texels
/// (Elevated: 226 bricks of an atlas 144 x 136; neighbouring bricks agree
/// on their shared border texel); the low half is `0xffff` on every map
/// read. The cell covers 8 probe texels, 48 units each, of the box
/// ([`Grids::box_min`]).
#[derive(Clone, Debug, Default)]
pub struct BrickTable {
    pub width: usize,
    pub height: usize,
    /// Per cell, row by row: the brick's (column, row) in the atlas.
    pub bricks: Vec<Option<(u8, u8)>>,
}

impl Grids {
    /// The brick table, where the map has an indoor probe volume with
    /// bricks (Rust's table is all empty; Farm 18 and Afghan have none).
    #[must_use]
    pub fn brick_table(&self) -> Option<BrickTable> {
        let g = self.grid_2d.as_ref()?;
        let (w, h) = (usize::from(g.width), usize::from(g.height));
        let cells = g.data.get(..w * h * 4)?;
        let bricks: Vec<Option<(u8, u8)>> = cells
            .chunks_exact(4)
            .map(|c| {
                let v = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                (v != u32::MAX).then_some((((v >> 16) & 0xff) as u8, ((v >> 24) & 0xff) as u8))
            })
            .collect();
        bricks.iter().any(Option::is_some).then_some(BrickTable { width: w, height: h, bricks })
    }
}

/// Everything a map zone gives of its own light.
#[derive(Clone, Debug, Default)]
pub struct MapLighting {
    pub probes: Vec<ReflectionProbe>,
    pub grids: Grids,
}

/// The back-pointer to asset `asset` of the list that starts at `base`.
fn image_ptr(base: u64, asset: usize) -> u64 {
    (0x0fu64 << 32) | (base + 16 * asset as u64 + 1)
}

/// The probe records: where each run whose k-th record carries the index
/// words of probe k starts, and how many records it has.
fn find_probe_records(z: &[u8]) -> Vec<(usize, usize)> {
    let first = 0xfff0_0000u32.to_le_bytes();
    let mut out = Vec::new();
    for hit in find_all(z, &first) {
        if hit < 0x30 {
            continue;
        }
        let start = hit - 0x30;
        let mut n = 0usize;
        while record_ok(z, start + PROBE_RECORD * n, n) {
            n += 1;
        }
        if n >= 2 {
            out.push((start, n));
        }
    }
    out
}

fn volume_image(z: &[u8], r: &ImageRec) -> Option<VolumeImage> {
    let (off, len) = r.inline?;
    Some(VolumeImage {
        hash: r.hash,
        width: r.width,
        height: r.height,
        depth: r.depth,
        array: r.array,
        format: r.format,
        usage: r.usage,
        data: z.get(off..off + len)?.to_vec(),
    })
}

/// Reads the map's probes and volume images. `run` is the zone's image run.
pub fn read(zone: &ZoneImage, run: &ImageRun, report: &mut Vec<String>) -> MapLighting {
    let z = &zone.bytes;
    let mut out = MapLighting::default();
    let recs: Vec<ImageRec> = run
        .at
        .iter()
        .filter_map(|&at| image::parse(z, at).ok().flatten())
        .collect();
    if recs.len() != run.at.len() {
        report.push("lighting: image headers did not all read".into());
        return out;
    }
    // The k-th image of the run is the k-th type-21 asset.
    let assets: Vec<usize> = zone.types.iter().enumerate().filter(|(_, t)| **t == 21).map(|(i, _)| i).collect();
    // The pointers' base (where block 0x0f's asset list starts differs by
    // zone): taken from the probes' pointer array, else the usual one.
    let mut base = zone.list_at as u64 - 0x14;
    // Probes: the records, each probe's picture pointed to by the array
    // of back-pointers just before them (in the records' order, which is
    // not always the pictures' order in the image run).
    let probe_idx: Vec<usize> = (0..recs.len())
        .filter(|&k| {
            let r = &recs[k];
            r.usage == PROBE_USE && r.format == 0x2d && r.width == r.height && r.depth == 1
        })
        .collect();
    if !probe_idx.is_empty() {
        let mut got = None;
        'runs: for (start, n) in find_probe_records(z) {
            let Some(arr) = start.checked_sub(8 * n) else { continue };
            let ptrs: Vec<u64> = (0..n).map(|k| u64_at(z, arr + 8 * k)).collect();
            if ptrs.iter().any(|p| p >> 32 != 0x0f || p & 1 == 0) {
                continue;
            }
            // The base that makes every pointer a probe picture's.
            for &cand in &probe_idx {
                let Some(b0) = (ptrs[0] & 0xffff_ffff).checked_sub(1 + 16 * assets[cand] as u64) else { continue };
                let picks: Option<Vec<usize>> = ptrs
                    .iter()
                    .map(|p| {
                        let d = (p & 0xffff_ffff).checked_sub(1 + b0)?;
                        (d % 16 == 0).then_some(())?;
                        probe_idx.iter().copied().find(|&k| assets[k] as u64 == d / 16)
                    })
                    .collect();
                if let Some(picks) = picks {
                    got = Some((start, n, b0, picks));
                    break 'runs;
                }
            }
        }
        match got {
            Some((start, n, b0, picks)) => {
                base = b0;
                for (k, &ri) in picks.iter().enumerate() {
                    let rec = start + PROBE_RECORD * k;
                    out.probes.push(ReflectionProbe {
                        index: k,
                        origin: [f32_at(z, rec + 0x10), f32_at(z, rec + 0x14), f32_at(z, rec + 0x18)],
                        image: super::materials::map_image(z, recs[ri].at),
                    });
                }
                let inline = out.probes.iter().filter(|p| p.image.inline.is_some()).count();
                report.push(format!(
                    "lighting: {n} reflection probes ({} {}x{} BC6H pictures, {inline} whole in the zone)",
                    probe_idx.len(),
                    recs[probe_idx[0]].width,
                    recs[probe_idx[0]].height
                ));
            }
            None => report.push(format!("lighting: {} probe pictures, no records pointing at them", probe_idx.len())),
        }
    } else {
        report.push("lighting: no reflection probe pictures".into());
    }
    let ptr_of = |k: usize| assets.get(k).map(|&a| image_ptr(base, a));
    // Volume images.
    let pick = |f: &dyn Fn(&ImageRec) -> bool| -> Option<usize> {
        let v: Vec<usize> = (0..recs.len()).filter(|&k| f(&recs[k])).collect();
        (v.len() == 1).then(|| v[0])
    };
    let vol = |k: Option<usize>| k.and_then(|k| volume_image(z, &recs[k]));
    let k_probe_volume = pick(&|r| r.usage == 0x21 && r.format == 0x2d && r.depth > 1);
    let k_six = pick(&|r| r.usage == 0x21 && r.array == 6 && r.format == 0x12);
    let k_mask = pick(&|r| r.usage == 0x21 && r.format == 0x2a && r.depth == 1);
    let k_b2 = pick(&|r| r.usage == 0x21 && r.format == 0x2b && r.depth == 1);
    let k_b3 = pick(&|r| r.usage == 0x21 && r.format == 0x2e && r.depth > 1);
    let k_idx3 = pick(&|r| r.usage == 0x21 && r.format == 0x23 && r.depth > 1);
    let k_g2 = pick(&|r| r.usage == 0x21 && r.format == 0x1b && r.depth == 1);
    out.grids = Grids {
        probe_volume: vol(k_probe_volume),
        grid_2d: vol(k_g2),
        index_3d: vol(k_idx3),
        six_way: vol(k_six),
        mask: vol(k_mask),
        bytes_2d: vol(k_b2),
        bytes_3d: vol(k_b3),
        ..Default::default()
    };
    // The header block with the four outdoor pointers [mask, 2-D bytes,
    // 3-D bytes, six-way], then the box (min and max, 16 bytes each) and
    // at +0xF0 the texel counts (as floats: 552, 528, 69, 66, 100, 92).
    if let (Some(m), Some(b2), Some(b3), Some(s)) = (k_mask, k_b2, k_b3, k_six)
        && let (Some(p0), Some(p1), Some(p2), Some(p3)) = (ptr_of(m), ptr_of(b2), ptr_of(b3), ptr_of(s))
    {
        let mut want = [0u8; 32];
        for (i, p) in [p0, p1, p2, p3].iter().enumerate() {
            want[8 * i..8 * i + 8].copy_from_slice(&p.to_le_bytes());
        }
        if let Some(at) = find_all(z, &want).into_iter().next() {
            let min = [f32_at(z, at + 0x20), f32_at(z, at + 0x24), f32_at(z, at + 0x28)];
            let max = [f32_at(z, at + 0x30), f32_at(z, at + 0x34), f32_at(z, at + 0x38)];
            let dims: [u32; 6] = std::array::from_fn(|i| {
                let v = f32_at(z, at + 0xF0 + 4 * i);
                if v.is_finite() && (0.0..65536.0).contains(&v) { v as u32 } else { 0 }
            });
            let sane = (0..3).all(|k| min[k].is_finite() && max[k].is_finite() && min[k] < max[k])
                && dims[0] == u32::from(recs[m].width)
                && dims[1] == u32::from(recs[m].height);
            if sane {
                out.grids.box_min = Some(min);
                out.grids.box_max = Some(max);
                out.grids.dims = Some(dims);
                out.grids.block_at = Some(at);
            }
        }
    }
    let have = [
        ("probe volume", out.grids.probe_volume.is_some()),
        ("index grid", out.grids.grid_2d.is_some()),
        ("index volume", out.grids.index_3d.is_some()),
        ("six-way grid", out.grids.six_way.is_some()),
        ("mask", out.grids.mask.is_some()),
        ("bytes 2-D", out.grids.bytes_2d.is_some()),
        ("bytes 3-D", out.grids.bytes_3d.is_some()),
    ];
    report.push(format!(
        "lighting: volume images {}; box {:?}",
        have.iter().filter(|h| h.1).map(|h| h.0).collect::<Vec<_>>().join(", "),
        out.grids.box_min.zip(out.grids.box_max)
    ));
    out
}

fn record_ok(z: &[u8], at: usize, k: usize) -> bool {
    at + PROBE_RECORD <= z.len()
        && u32_at(z, at + 0x30) == (0xfff0_0000 | k as u32)
        && (0..3).all(|c| f32_at(z, at + 0x10 + 4 * c).is_finite())
}
