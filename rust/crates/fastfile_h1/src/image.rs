//! Image pixels: an image's mip chain from the zone itself or, for a
//! streamed image, from the `imagefile<N>.pak` files beside the zones.
//!
//! A streamed image names four parts (`XStreamFile`s in the zone header).
//! Part 0 holds the smallest streamed size with all its smaller levels;
//! each later part adds the next larger level (`GfxImageStreamData`:
//! width, height and the cumulative byte count). Each part is packed in its
//! pak as the zone data is: u64 size, u32 flags, then LZ4 blocks
//! (`container::inflate_blocks`; read on retail `imagefile1.pak`).
//!
//! A zone built by the zone tools (Horizon MW usermaps) keeps its own
//! streamed images in `<zone>.pak` beside the `.ff`, and names it with file
//! index [`ZONE_PAK_INDEX`] (96; x64-zt `h1/zone.cpp`
//! `CUSTOM_IMAGEFILE_INDEX`). Its parts are packed the same way. Its other
//! streamed images name the retail `imagefile<N>.pak`s by their index.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::container::StreamFile;
use crate::walk::ImageRec;

/// One image's pixels, levels largest first, in its DXGI format.
#[derive(Clone, Debug)]
pub struct ImagePixels {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub dxgi_format: u32,
    pub data: Vec<u8>,
}

/// Bytes per 4x4 block (block formats) or per pixel, and whether the
/// format is block compressed.
pub fn dxgi_unit(format: u32) -> Option<(usize, bool)> {
    Some(match format {
        // BC1, BC4: 8 bytes a block.
        70..=72 | 79..=81 => (8, true),
        // BC2, BC3, BC5, BC6H, BC7: 16.
        73..=78 | 82..=84 | 94..=99 => (16, true),
        // R8G8B8A8, B8G8R8A8 / X8, R10G10B10A2, R16G16, R32: 4 bytes.
        27..=32 | 87..=93 | 23..=26 | 33..=38 | 39..=43 => (4, false),
        // R32G32B32A32: 16.
        1..=4 => (16, false),
        // R16G16B16A16, R32G32: 8.
        9..=14 | 15..=18 => (8, false),
        // R8G8, R16: 2.
        48..=52 | 53..=59 => (2, false),
        // R8, A8: 1.
        60..=65 => (1, false),
        _ => return None,
    })
}

/// Bytes of one level of `w` x `h`.
pub fn level_bytes(format: u32, w: u32, h: u32) -> Option<usize> {
    let (unit, block) = dxgi_unit(format)?;
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    Some(if block {
        w.div_ceil(4) * h.div_ceil(4) * unit
    } else {
        w * h * unit
    })
}

/// The file index the zone tools give a zone's own pak (`<zone>.pak`).
pub const ZONE_PAK_INDEX: u16 = 96;

static EXTRA_PAKS: std::sync::Mutex<Vec<(u16, PathBuf)>> = std::sync::Mutex::new(Vec::new());

/// Names the pak of a guest zone read for its pictures (file index `idx`).
pub fn register_extra_pak(idx: u16, path: PathBuf) {
    if let Ok(mut v) = EXTRA_PAKS.lock() {
        v.retain(|(i, _)| *i != idx);
        v.push((idx, path));
    }
}

/// The `imagefile<N>.pak` files beside a zone, opened as needed.
pub struct ImagePaks {
    dir: PathBuf,
    /// The zone's own pak (file index [`ZONE_PAK_INDEX`]), if known.
    zone_pak: Option<PathBuf>,
    open: HashMap<u16, Option<File>>,
}

impl ImagePaks {
    /// The retail paks in `zone_dir` (`imagefile<N>.pak`).
    pub fn new(zone_dir: &Path) -> Self {
        Self {
            dir: zone_dir.to_path_buf(),
            zone_pak: None,
            open: HashMap::new(),
        }
    }

    /// The paks a zone file's images name: its own `<zone>.pak` beside it
    /// for index 96, the retail `imagefile<N>.pak` in `retail_dir` (or
    /// beside the zone) for the others.
    pub fn for_zone(zone_path: &Path, retail_dir: Option<&Path>) -> Self {
        let beside = zone_path.parent().unwrap_or(Path::new("."));
        // (A zone reached through a link has its pak beside the file
        // itself.)
        let own = zone_path.with_extension("pak");
        let own = if own.is_file() {
            own
        } else {
            std::fs::canonicalize(zone_path)
                .map(|p| p.with_extension("pak"))
                .ok()
                .filter(|p| p.is_file())
                .unwrap_or(own)
        };
        Self {
            dir: retail_dir.unwrap_or(beside).to_path_buf(),
            zone_pak: Some(own),
            open: HashMap::new(),
        }
    }

    /// The file a streamed part names.
    pub fn path_of(&self, file_index: u16) -> PathBuf {
        match (&self.zone_pak, file_index) {
            (Some(p), ZONE_PAK_INDEX) => p.clone(),
            (_, i) if i >= 1000 => EXTRA_PAKS
                .lock()
                .ok()
                .and_then(|v| v.iter().find(|(k, _)| *k == i).map(|(_, p)| p.clone()))
                .unwrap_or_else(|| self.dir.join(format!("imagefile{i}.pak"))),
            _ => self.dir.join(format!("imagefile{file_index}.pak")),
        }
    }

    /// One streamed part, inflated; `None` when its pak is missing or the
    /// part does not read.
    pub fn read(&mut self, part: &StreamFile, expect: usize) -> Option<Vec<u8>> {
        if part.offset == 0 || part.offset_end <= part.offset {
            return None;
        }
        let path = self.path_of(part.file_index);
        let file = self
            .open
            .entry(part.file_index)
            .or_insert_with(|| File::open(path).ok())
            .as_mut()?;
        file.seek(SeekFrom::Start(part.offset)).ok()?;
        let mut packed = vec![0u8; (part.offset_end - part.offset) as usize];
        file.read_exact(&mut packed).ok()?;
        let mut out = crate::container::inflate_blocks(&packed).ok()?;
        (out.len() >= expect).then(|| {
            out.truncate(expect);
            out
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// An image's 2-D pixels, its largest level no bigger than `max_size` on a
/// side (larger levels are left out; at least the smallest level kept).
pub fn image_pixels(
    rec: &ImageRec,
    zone_image: &[u8],
    paks: &mut ImagePaks,
    max_size: u32,
) -> Result<ImagePixels, String> {
    let format = rec.format;
    // The engine's own flat normal map (`$identitynormalmap`) has no pixels
    // and no format in the zone: the renderer makes it. It is (0.5, 0.5, 1).
    if format == 0 && rec.name.ends_with("$identitynormalmap") {
        return Ok(ImagePixels {
            width: 1,
            height: 1,
            levels: 1,
            dxgi_format: 28, // R8G8B8A8_UNORM
            data: vec![128, 128, 255, 255],
        });
    }
    dxgi_unit(format).ok_or_else(|| format!("DXGI format {format} is not read"))?;
    if rec.streamed {
        return streamed(rec, paks, max_size);
    }
    let pixels = rec.pixels.ok_or("no pixels in the zone")?;
    let data = pixels.get(zone_image);
    let (mut w, mut h) = (u32::from(rec.width), u32::from(rec.height));
    let levels = u32::from(rec.level_count.max(1));
    let mut at = 0usize;
    let mut kept = levels;
    // Leave out levels above the size cap.
    while kept > 1 && w.max(h) > max_size {
        at += level_bytes(format, w, h).unwrap_or(0);
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        kept -= 1;
    }
    let mut len = 0usize;
    let (mut lw, mut lh) = (w, h);
    for _ in 0..kept {
        len += level_bytes(format, lw, lh).unwrap_or(0);
        lw = (lw / 2).max(1);
        lh = (lh / 2).max(1);
    }
    let bytes = data
        .get(at..at + len)
        .ok_or_else(|| format!("{} bytes in the zone, {} wanted", data.len(), at + len))?;
    Ok(ImagePixels {
        width: w,
        height: h,
        levels: kept,
        dxgi_format: format,
        data: bytes.to_vec(),
    })
}

fn streamed(rec: &ImageRec, paks: &mut ImagePaks, max_size: u32) -> Result<ImagePixels, String> {
    let format = rec.format;
    // The largest part that fits the cap, with every smaller part readable.
    let parts: Vec<usize> = (0..4)
        .filter(|&i| rec.streams[i].0 > 0 && rec.stream_files[i].is_some())
        .collect();
    let Some(&top) = parts
        .iter()
        .rev()
        .find(|&&i| u32::from(rec.streams[i].0.max(rec.streams[i].1)) <= max_size)
        .or(parts.first())
    else {
        return Err("no streamed parts".into());
    };
    let mut chunks = Vec::new();
    for i in (0..=top).rev() {
        let (_, _, cum, _) = rec.streams[i];
        let before = if i == 0 { 0 } else { rec.streams[i - 1].2 };
        let want = (cum.saturating_sub(before)) as usize;
        let part = rec.stream_files[i].ok_or_else(|| format!("part {i} has no pak range"))?;
        let bytes = paks.read(&part, want).ok_or_else(|| {
            format!(
                "part {i} not read from {}",
                paks.path_of(part.file_index).display()
            )
        })?;
        chunks.push(bytes);
    }
    let (w, h, _, _) = rec.streams[top];
    // One level from each part above part 0; part 0 its whole chain.
    let levels = top as u32 + u32::from(rec.streams[0].3.max(1));
    Ok(ImagePixels {
        width: u32::from(w),
        height: u32::from(h),
        levels,
        dxgi_format: format,
        data: chunks.concat(),
    })
}

/// An image's largest level as RGBA8 bytes (BC1, BC2, BC3, BC4, BC7 and the
/// 8-bit formats; BC4's one channel spread over red, green and blue).
pub fn rgba8_level0(px: &ImagePixels) -> Result<(u32, u32, Vec<u8>), String> {
    let (w, h) = (px.width as usize, px.height as usize);
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    let mut out = vec![255u8; w * h * 4];
    let need = level_bytes(px.dxgi_format, px.width, px.height).ok_or("format not read")?;
    let data = px.data.get(..need).ok_or("short level")?;
    match px.dxgi_format {
        70..=72 | 73..=75 | 76..=78 | 79..=81 | 97..=99 => {
            if w % 4 != 0 || h % 4 != 0 {
                return Err(format!("{w}x{h} is not whole blocks"));
            }
            let unit = dxgi_unit(px.dxgi_format).map_or(16, |u| u.0);
            let mut grey = [0u8; 16];
            for by in 0..h / 4 {
                for bx in 0..w / 4 {
                    let b = &data[(by * (w / 4) + bx) * unit..][..unit];
                    let at = ((by * 4) * w + bx * 4) * 4;
                    match px.dxgi_format {
                        70..=72 => bcdec_rs::bc1(b, &mut out[at..], w * 4),
                        73..=75 => bcdec_rs::bc2(b, &mut out[at..], w * 4),
                        76..=78 => bcdec_rs::bc3(b, &mut out[at..], w * 4),
                        97..=99 => bcdec_rs::bc7(b, &mut out[at..], w * 4),
                        _ => {
                            bcdec_rs::bc4(b, &mut grey, 4, px.dxgi_format == 81);
                            for y in 0..4 {
                                for x in 0..4 {
                                    let g = grey[y * 4 + x];
                                    let o = at + (y * w + x) * 4;
                                    out[o..o + 4].copy_from_slice(&[g, g, g, 255]);
                                }
                            }
                        }
                    }
                }
            }
        }
        27..=29 => out.copy_from_slice(&data[..w * h * 4]),
        87 | 90 | 91 => {
            for (o, i) in out.chunks_exact_mut(4).zip(data.chunks_exact(4)) {
                o.copy_from_slice(&[
                    i[2],
                    i[1],
                    i[0],
                    if px.dxgi_format == 87 { i[3] } else { 255 },
                ]);
            }
        }
        60..=63 => {
            for (o, &g) in out.chunks_exact_mut(4).zip(data) {
                o.copy_from_slice(&[g, g, g, 255]);
            }
        }
        f => return Err(format!("DXGI format {f} is not decoded")),
    }
    Ok((px.width, px.height, out))
}

/// `colour` with its alpha replaced by `mask`'s red (resampled to the
/// colour's size): a reveal decal's picture, the reveal map in its alpha.
/// RGBA8 (`R8G8B8A8_UNORM_SRGB`) with a full chain of box-filtered levels.
pub fn with_alpha_of(colour: &ImagePixels, mask: &ImagePixels) -> Result<ImagePixels, String> {
    let (w, h, mut rgba) = rgba8_level0(colour)?;
    let (mw, mh, m) = rgba8_level0(mask)?;
    let (w, h, mw, mh) = (w as usize, h as usize, mw as usize, mh as usize);
    for y in 0..h {
        let fy = ((y as f32 + 0.5) * mh as f32 / h as f32 - 0.5).max(0.0);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(mh - 1);
        let y0 = y0.min(mh - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * mw as f32 / w as f32 - 0.5).max(0.0);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(mw - 1);
            let x0 = x0.min(mw - 1);
            let at = |xx: usize, yy: usize| f32::from(m[(yy * mw + xx) * 4]);
            let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
            let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
            rgba[(y * w + x) * 4 + 3] = (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(rgba8_chain(w, h, rgba, 29))
}

/// A packed specular picture (Horizon MW's `os0` materials: red the
/// specular strength, green the gloss, blue the occlusion; the game's
/// `..._s0om0os0` shaders read them so) as a plain one: the strength in red,
/// green and blue, the gloss in alpha. RGBA8 with its levels.
pub fn unpack_spec(px: &ImagePixels) -> Result<ImagePixels, String> {
    let (w, h, mut rgba) = rgba8_level0(px)?;
    for t in rgba.chunks_exact_mut(4) {
        let (s, g) = (t[0], t[1]);
        t.copy_from_slice(&[s, s, s, g]);
    }
    Ok(rgba8_chain(w as usize, h as usize, rgba, 28))
}

/// `px` with its red, green and blue all `rgb` (its alpha kept): Modern
/// Warfare Remastered's specular maps built over `$white` keep only their
/// gloss (alpha); RGBA8 (`R8G8B8A8_UNORM`) with its levels.
pub fn with_rgb(px: &ImagePixels, rgb: [u8; 3]) -> Result<ImagePixels, String> {
    let (w, h, mut rgba) = rgba8_level0(px)?;
    for t in rgba.chunks_exact_mut(4) {
        t[..3].copy_from_slice(&rgb);
    }
    Ok(rgba8_chain(w as usize, h as usize, rgba, 28))
}

/// An RGBA8 picture with every smaller level, each the box filter of the
/// one above.
fn rgba8_chain(w: usize, h: usize, rgba: Vec<u8>, dxgi_format: u32) -> ImagePixels {
    let mut data = rgba.clone();
    let (mut lw, mut lh, mut cur) = (w, h, rgba);
    let mut levels = 1;
    while lw > 1 || lh > 1 {
        let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(lw - 1), (2 * y + dy).min(lh - 1));
                        s += u32::from(cur[(sy * lw + sx) * 4 + c]);
                    }
                    next[(y * nw + x) * 4 + c] = ((s + 2) / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        (lw, lh, cur) = (nw, nh, next);
        levels += 1;
    }
    ImagePixels {
        width: w as u32,
        height: h as u32,
        levels,
        dxgi_format,
        data,
    }
}

#[cfg(test)]
mod reveal_tests {
    use super::*;

    #[test]
    fn two_channel_16_bit_formats_are_read() {
        // The split-sum lookups (`~envbrdflut_ggx_16-rg`) are R16G16_FLOAT.
        for f in 33..=38 {
            assert_eq!(dxgi_unit(f), Some((4, false)), "format {f}");
        }
        assert_eq!(level_bytes(34, 64, 64), Some(64 * 64 * 4));
        assert_eq!(dxgi_unit(2), Some((16, false)));
        assert_eq!(dxgi_unit(16), Some((8, false)));
    }

    #[test]
    fn a_mask_lands_in_the_colour_alpha_with_levels() {
        let colour = ImagePixels {
            width: 4,
            height: 4,
            levels: 1,
            dxgi_format: 28,
            data: vec![200; 64],
        };
        let mask = ImagePixels {
            width: 2,
            height: 2,
            levels: 1,
            dxgi_format: 61,
            data: vec![0, 255, 0, 255],
        };
        let out = with_alpha_of(&colour, &mask).unwrap();
        assert_eq!(out.levels, 3);
        assert_eq!(out.data.len(), (16 + 4 + 1) * 4);
        assert_eq!(out.data[3], 0);
        assert_eq!(out.data[3 * 4 + 3], 255);
        assert_eq!(out.data[0], 200);
    }
}
