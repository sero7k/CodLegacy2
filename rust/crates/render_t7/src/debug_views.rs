//! `BO2MP_T7_NATIVE_DEBUG=<mode>` views (documented in `docs/T7_NATIVE.md`,
//! "Debug views"). The cheap ones change what the lighting globals or the
//! G-buffer draw ([`mode`] is read by `lighting_globals` and `gbuffer`); the
//! readback ones (`tiles`, `cullbits`, `nan`, `exposure`) run once, a few
//! frames in, over what the lighting stage left on the device and put their
//! findings in the log (and, with `BO2MP_T7_DEBUG_PNG=<file>`, a picture).

use render_h1::gpu::Gpu;
use crate::lighting::Lighting;

/// The lowercased `BO2MP_T7_NATIVE_DEBUG` ("" unset).
pub fn mode() -> &'static str {
    static M: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    M.get_or_init(|| std::env::var("BO2MP_T7_NATIVE_DEBUG").map(|v| v.to_ascii_lowercase()).unwrap_or_default())
}

/// `BO2MP_T7_SEAMS=1`: the `empty` readback whatever the mode (to read it under `sunonly` etc.).
pub fn seams_on() -> bool {
    std::env::var("BO2MP_T7_SEAMS").is_ok_and(|v| v != "0")
}

/// `BO2MP_T7_KEEP_FORWARD=1`: the `empty` readback with the forward lists (the sky) drawn.
pub fn keep_forward() -> bool {
    std::env::var("BO2MP_T7_KEEP_FORWARD").is_ok_and(|v| v != "0")
}

/// The modes that read the device back once.
pub fn wants_readback() -> bool {
    matches!(mode(), "tiles" | "cullbits" | "nan" | "exposure" | "blackpatch" | "empty") || seams_on()
}

/// The frame the readback runs on (the earlier ones submitted).
pub fn readback_frame() -> u32 {
    std::env::var("BO2MP_T7_DEBUG_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(30)
}

/// A MAP_READ buffer's bytes once the GPU is done with it; zeros when the
/// copy or the map failed (a debug readback must never end the game).
pub(crate) fn mapped_bytes(gpu: &Gpu, rb: &wgpu::Buffer) -> Vec<u8> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let ok = std::sync::Arc::new(AtomicBool::new(false));
    let ok2 = ok.clone();
    rb.slice(..).map_async(wgpu::MapMode::Read, move |r| ok2.store(r.is_ok(), Ordering::Release));
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    if !ok.load(Ordering::Acquire) {
        return vec![0; rb.size() as usize];
    }
    let v = rb.slice(..).get_mapped_range().to_vec();
    rb.unmap();
    v
}

fn read(gpu: &Gpu, b: &wgpu::Buffer, n: u64) -> Vec<u8> {
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: n, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(b, 0, &rb, 0, n);
    gpu.queue.submit([enc.finish()]);
    let v = mapped_bytes(gpu, &rb);
    v
}

fn u32s(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn f16(h: u16) -> f32 {
    let (s, e, m) = (u32::from(h >> 15), u32::from((h >> 10) & 0x1f), u32::from(h & 0x3ff));
    let v = match e {
        0 => m as f32 * 2f32.powi(-24),
        31 => {
            if m == 0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + m as f32 / 1024.0) * 2f32.powi(e as i32 - 15),
    };
    if s == 1 { -v } else { v }
}

/// The lit scene's pixels (RGBA16F) as rows of `[r, g, b, a]` f32.
fn read_scene(gpu: &Gpu, t: &wgpu::Texture) -> Vec<[f32; 4]> {
    let (w, h) = (t.width(), t.height());
    let row = (w * 8).div_ceil(256) * 256;
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } }, t.size());
    gpu.queue.submit([enc.finish()]);
    let data = mapped_bytes(gpu, &rb);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let o = y * row as usize + x * 8;
            let c = |k: usize| f16(u16::from_le_bytes([data[o + 2 * k], data[o + 2 * k + 1]]));
            out.push([c(0), c(1), c(2), c(3)]);
        }
    }
    out
}

/// What the mode reads back, as log lines (all lines start `debug:`).
pub fn report(gpu: &Gpu, lit: &Lighting) -> Vec<String> {
    let mut out = Vec::new();
    let (Some(tex), Some((tiling, cull))) = (lit.scene_texture(), lit.debug_buffers()) else {
        return vec!["debug: the lighting stage is not sized yet".into()];
    };
    let (acc, _ind, ext) = lit.permute_buffers();
    let (w, h) = (tex.width(), tex.height());
    let (tw, th) = (w.div_ceil(8), h.div_ceil(8));
    let tiles = (tw * th) as usize;
    let counts = u32s(&read(gpu, acc, 256));
    out.push(format!(
        "debug: {w}x{h}, {tw}x{th} tiles; tiles per permutation: {:?}",
        counts.iter().enumerate().filter(|(_, c)| **c > 0).map(|(i, c)| (i, *c)).collect::<Vec<_>>()
    ));
    // Which permutation each tile took: (x = word 0 & 0xfff, y = word 1 &
    // 0xfff, as `deferred_permute_classify_swizzle` packs them).
    let mut perm_of = vec![u16::MAX; tiles];
    let list = u32s(&read(gpu, tiling, 8 * tiles as u64 * 64));
    let mut dup = 0usize;
    for (p, &n) in counts.iter().enumerate() {
        for i in 0..(n as usize).min(tiles) {
            let (a, b) = (list[(p * tiles + i) * 2], list[(p * tiles + i) * 2 + 1]);
            let (x, y) = ((a & 0xfff) as usize, (b & 0xfff) as usize);
            if x < tw as usize && y < th as usize {
                let t = y * tw as usize + x;
                dup += usize::from(perm_of[t] != u16::MAX);
                perm_of[t] = p as u16;
            }
        }
    }
    let unlisted = perm_of.iter().filter(|&&p| p == u16::MAX).count();
    out.push(format!("debug: {unlisted} of {tiles} tiles are in no permutation's list; {dup} are in more than one"));
    // The tile depth extents.
    let ex: Vec<f32> = ext.map(|e| u32s(&read(gpu, e, 8 * tiles as u64)).into_iter().map(f32::from_bits).collect()).unwrap_or_default();
    // The lit scene.
    let px = read_scene(gpu, tex);
    let (mut nan, mut zero, mut sum, mut max) = (0usize, 0usize, 0f64, 0f32);
    let mut tile_nan = vec![0u16; tiles];
    let mut tile_zero = vec![0u16; tiles];
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i % w as usize, i / w as usize);
        let t = (y / 8) * tw as usize + x / 8;
        if p.iter().any(|v| !v.is_finite()) {
            nan += 1;
            tile_nan[t] += 1;
        } else if p[0] == 0.0 && p[1] == 0.0 && p[2] == 0.0 {
            zero += 1;
            tile_zero[t] += 1;
        } else {
            let l = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
            sum += f64::from(l);
            max = max.max(l);
        }
    }
    let n = px.len().max(1);
    let finite = (n - nan - zero).max(1);
    out.push(format!(
        "debug: lit scene: {:.2}% NaN/Inf, {:.2}% exactly black; mean luminance of the rest {:.4} (x relHDRExposure.y = {:.4} display), max {:.1}",
        100.0 * nan as f64 / n as f64,
        100.0 * zero as f64 / n as f64,
        sum / finite as f64,
        sum / finite as f64 * f64::from(lit.exposure_scale().1),
        max
    ));
    let full = |v: &[u16]| v.iter().filter(|&&c| c >= 64).count();
    let (fn_, fz) = (full(&tile_nan), full(&tile_zero));
    out.push(format!("debug: tiles wholly NaN/Inf: {fn_}; wholly black: {fz}; tiles with any NaN/Inf: {}", tile_nan.iter().filter(|&&c| c > 0).count()));
    // The black/NaN tiles by permutation and depth extents.
    let mut by_perm: std::collections::BTreeMap<u16, (usize, usize, usize)> = Default::default();
    for t in 0..tiles {
        let e = by_perm.entry(perm_of[t]).or_default();
        e.0 += 1;
        e.1 += usize::from(tile_nan[t] >= 64);
        e.2 += usize::from(tile_zero[t] >= 64);
    }
    for (p, (all, nn, zz)) in &by_perm {
        let pn = if *p == u16::MAX { "none".to_string() } else { p.to_string() };
        out.push(format!("debug: permutation {pn}: {all} tiles, {nn} wholly NaN/Inf, {zz} wholly black"));
    }
    let mut shown = 0;
    for t in 0..tiles {
        if (tile_nan[t] >= 64 || tile_zero[t] >= 64) && shown < 12 {
            let (mn, mx) = (ex.get(2 * t).copied().unwrap_or(f32::NAN), ex.get(2 * t + 1).copied().unwrap_or(f32::NAN));
            out.push(format!("debug: bad tile ({}, {}) perm {} depth extents [{mn:e}, {mx:e}] nan {} black {}", t % tw as usize, t / tw as usize, perm_of[t] as i32, tile_nan[t], tile_zero[t]));
            shown += 1;
        }
    }
    // The cull bits: per tile, how many of the bits are set.
    if matches!(mode(), "cullbits" | "tiles" | "blackpatch") {
        let words = (cull.size() / 4) as usize / tiles.max(1);
        let bits = u32s(&read(gpu, cull, (4 * words * tiles) as u64));
        let pop: Vec<u32> = (0..tiles).map(|t| bits[t * words..(t + 1) * words].iter().map(|w| w.count_ones()).sum()).collect();
        let (mn, mx) = (pop.iter().min().copied().unwrap_or(0), pop.iter().max().copied().unwrap_or(0));
        out.push(format!(
            "debug: cull bits: {words} words a tile; set bits a tile min {mn}, max {mx}, mean {:.1}; {} tiles with none",
            pop.iter().map(|&c| f64::from(c)).sum::<f64>() / tiles.max(1) as f64,
            pop.iter().filter(|&&c| c == 0).count()
        ));
        for t in 0..tiles {
            if (tile_nan[t] >= 64 || tile_zero[t] >= 64) && shown < 20 {
                out.push(format!("debug: bad tile ({}, {}) cull bits set {} (words {:08x?})", t % tw as usize, t / tw as usize, pop[t], &bits[t * words..t * words + words.min(4)]));
                shown += 1;
            }
        }
    }
    // The picture: permutation colour (dim), NaN/Inf magenta, black cyan.
    if let Ok(path) = std::env::var("BO2MP_T7_DEBUG_PNG") {
        let mut rgb = Vec::with_capacity(px.len() * 3);
        for (i, p) in px.iter().enumerate() {
            let (x, y) = (i % w as usize, i / w as usize);
            let t = (y / 8) * tw as usize + x / 8;
            let c: [u8; 3] = if p.iter().any(|v| !v.is_finite()) {
                [255, 0, 255]
            } else if p[0] == 0.0 && p[1] == 0.0 && p[2] == 0.0 {
                [0, 255, 255]
            } else {
                let q = u32::from(perm_of[t]).wrapping_mul(2654435761);
                [(64 + (q >> 24) % 128) as u8, (64 + (q >> 16) % 128) as u8, (64 + (q >> 8) % 128) as u8]
            };
            rgb.extend_from_slice(&c);
        }
        match std::fs::File::create(&path) {
            Ok(f) => {
                let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                e.set_color(png::ColorType::Rgb);
                e.set_depth(png::BitDepth::Eight);
                match e.write_header().and_then(|mut wr| wr.write_image_data(&rgb)) {
                    Ok(()) => out.push(format!("debug: wrote {path} (permutation colours; magenta NaN/Inf; cyan exactly black)")),
                    Err(er) => out.push(format!("debug: {path}: {er}")),
                }
            }
            Err(er) => out.push(format!("debug: {path}: {er}")),
        }
    }
    out
}

pub(crate) fn read_tex(gpu: &Gpu, t: &wgpu::Texture, aspect: wgpu::TextureAspect, bpp: u32) -> Vec<u32> {
    let (w, h) = (t.width(), t.height());
    let row = (w * bpp).div_ceil(256) * 256;
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    let mut src = t.as_image_copy();
    src.aspect = aspect;
    enc.copy_texture_to_buffer(src, wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } }, t.size());
    gpu.queue.submit([enc.finish()]);
    let data = mapped_bytes(gpu, &rb);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let o = y * row as usize + x * bpp as usize;
            out.push(u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]));
        }
    }
    out
}


/// The permutation each 8x8 tile took (`u16::MAX`: in no list).
fn perm_map(gpu: &Gpu, lit: &Lighting, w: usize, h: usize) -> Option<Vec<u16>> {
    let (tiling, _cull) = lit.debug_buffers()?;
    let (acc, _ind, _ext) = lit.permute_buffers();
    let (tw, th) = (w.div_ceil(8), h.div_ceil(8));
    let tiles = tw * th;
    let counts = u32s(&read(gpu, acc, 256));
    let mut perm_of = vec![u16::MAX; tiles];
    let list = u32s(&read(gpu, tiling, 8 * tiles as u64 * 64));
    for (p, &n) in counts.iter().enumerate() {
        for i in 0..(n as usize).min(tiles) {
            let (a, b) = (list[(p * tiles + i) * 2], list[(p * tiles + i) * 2 + 1]);
            let (x, y) = ((a & 0xfff) as usize, (b & 0xfff) as usize);
            if x < tw && y < th {
                perm_of[y * tw + x] = p as u16;
            }
        }
    }
    Some(perm_of)
}

/// `BO2MP_T7_NATIVE_DEBUG=empty`: the pixels with no surface (depth 0, the
/// reversed far plane) and what the lit scene holds there; with
/// `BO2MP_T7_DEBUG_PNG=<file>` a picture (grey: surface, black: empty and
/// clean, red: empty but lit). Run without the sky (offline: no
/// `T7_SYNTHETIC_SKY_MESH`) the lit scene must be exactly zero there.
pub fn empty_report(gpu: &Gpu, lit: &Lighting, targets: &crate::gbuffer::Targets) -> Vec<String> {
    let Some(tex) = lit.scene_texture() else { return vec!["debug: empty: the lighting stage is not sized yet".into()] };
    let (w, h) = (tex.width() as usize, tex.height() as usize);
    let depth: Vec<f32> = read_tex(gpu, &targets.depth.texture, wgpu::TextureAspect::DepthOnly, 4).into_iter().map(f32::from_bits).collect();
    let normal = read_tex(gpu, &targets.colour[1].texture, wgpu::TextureAspect::All, 4);
    let px = read_scene(gpu, tex);
    let (mut empty, mut lit_empty, mut nz_empty, mut surf_black) = (0usize, 0usize, 0usize, 0usize);
    let mut tiles_bad = std::collections::BTreeSet::new();
    let perm_of = perm_map(gpu, lit, w, h);
    let tw = w.div_ceil(8);
    let (mut black_unlisted, mut black_listed) = (0usize, 0usize);
    let mut black_perm: std::collections::BTreeMap<u16, usize> = Default::default();
    let (mut black_d, mut lit_d) = ((f32::MAX, 0f32), (f32::MAX, 0f32));
    let mut perm_lum: std::collections::BTreeMap<u16, (f64, usize)> = Default::default();
    let mut img = vec![0u8; w * h * 3];
    for i in 0..w * h {
        let p = px[i];
        let on = p[0] != 0.0 || p[1] != 0.0 || p[2] != 0.0;
        let nzz = (normal[i] >> 20) & 0x3ff;
        if depth[i] == 0.0 {
            empty += 1;
            nz_empty += usize::from(nzz > 0);
            if on {
                lit_empty += 1;
                tiles_bad.insert((i % w / 8, i / w / 8));
                img[i * 3] = 255;
            }
        } else {
            surf_black += usize::from(!on);
            let r = if on { &mut lit_d } else { &mut black_d };
            r.0 = r.0.min(depth[i]);
            r.1 = r.1.max(depth[i]);
            if let Some(pm) = perm_of.as_ref() {
                let e = perm_lum.entry(pm[i / w / 8 * tw + i % w / 8]).or_default();
                e.0 += f64::from(0.2126 * px[i][0] + 0.7152 * px[i][1] + 0.0722 * px[i][2]);
                e.1 += 1;
            }
            if !on && let Some(pm) = perm_of.as_ref() {
                let pr = pm[i / w / 8 * tw + i % w / 8];
                if pr == u16::MAX {
                    black_unlisted += 1;
                } else {
                    black_listed += 1;
                    *black_perm.entry(pr).or_default() += 1;
                }
            }
            if on {
                img[i * 3..i * 3 + 3].copy_from_slice(&[110, 110, 110]);
            } else {
                img[i * 3..i * 3 + 3].copy_from_slice(&[0, 90, 255]);
            }
        }
    }
    // Tile seams: neighbouring surface pixels on one surface (depth within 0.3%, same normal word)
    // whose lit luminance jumps across an 8x8 tile border, against the same pairs inside a tile.
    let lum = |p: &[f32; 4]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let (mut seam_n, mut seam_sum, mut seam_big, mut in_n, mut in_sum, mut in_big) = (0usize, 0f64, 0usize, 0usize, 0f64, 0usize);
    let mut worst: Vec<(f32, usize, usize)> = Vec::new();
    let mut pair_stats: std::collections::BTreeMap<(u16, u16), (usize, usize, f64, usize)> = Default::default();
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            for vertical in [false, true] {
            let (a, b) = if vertical { (y * w + x, (y + 1) * w + x) } else { (y * w + x, y * w + x + 1) };
            if depth[a] == 0.0 || depth[b] == 0.0 || normal[a] != normal[b] || (depth[a] - depth[b]).abs() > 0.003 * depth[a] {
                continue;
            }
            let (la, lb) = (lum(&px[a]).max(1e-4), lum(&px[b]).max(1e-4));
            let r = (la / lb).max(lb / la);
            if (if vertical { y } else { x }) % 8 == 7 {
                if let Some(pm) = perm_of.as_ref() {
                    let (pa, pb) = if vertical { (pm[y / 8 * tw + x / 8], pm[(y + 1) / 8 * tw + x / 8]) } else { (pm[y / 8 * tw + x / 8], pm[y / 8 * tw + (x + 1) / 8]) };
                    if pa != pb {
                        let zero = lum(&px[a]) == 0.0 || lum(&px[b]) == 0.0;
                        let e = pair_stats.entry((pa.min(pb), pa.max(pb))).or_insert((0usize, 0usize, 0f64, 0usize));
                        e.0 += 1;
                        if zero {
                            e.1 += 1;
                        } else {
                            e.2 += f64::from(r.ln());
                            e.3 += 1;
                        }
                    }
                }
                seam_n += 1;
                seam_sum += f64::from(r.ln());
                seam_big += usize::from(r > 1.25);
                if r > 1.25 {
                    worst.push((r, x, y));
                }
            } else {
                in_n += 1;
                in_sum += f64::from(r.ln());
                in_big += usize::from(r > 1.25);
            }
            }
        }
    }
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let mut out = vec![format!(
        "debug: seams: same-surface pixel pairs across a tile border {seam_n}: mean |ln ratio| {:.4}, {} ({:.2}%) over 1.25x; inside tiles {in_n}: mean {:.4}, {} ({:.2}%) over 1.25x",
        seam_sum / seam_n.max(1) as f64, seam_big, 100.0 * seam_big as f64 / seam_n.max(1) as f64,
        in_sum / in_n.max(1) as f64, in_big, 100.0 * in_big as f64 / in_n.max(1) as f64
    )];
    let mut ps: Vec<_> = pair_stats.iter().collect();
    ps.sort_by_key(|(_, v)| std::cmp::Reverse(v.0));
    for ((a, b), (n, z, sum, nn)) in ps.iter().take(12) {
        out.push(format!("debug: seams: permutations {a} | {b}: {n} pairs, {z} with a black side, mean |ln ratio| of the rest {:.3}", sum / (*nn).max(1) as f64));
    }
    for (r, x, y) in worst.iter().take(5) {
        out.push(format!("debug: seams: worst ({x}, {y}) ratio {r:.2}: {:?} | {:?}", px[y * w + x], px[y * w + x + 1]));
    }
    out.push(format!(
        "debug: empty: {w}x{h}: {empty} pixels with no surface (depth 0), {lit_empty} of them lit in {} 8x8 tiles, {nz_empty} with normal.z > 0; {surf_black} surface pixels exactly black",
        tiles_bad.len()
    ));
    out.push(format!("debug: empty: black surface pixels: {black_unlisted} in tiles of no permutation list, {black_listed} in listed tiles (by permutation {black_perm:?})"));
    out.push(format!("debug: empty: mean lit luminance of surface pixels by permutation: {:?}", perm_lum.iter().map(|(k, v)| (*k, v.1, (v.0 / v.1 as f64 * 1e4).round() / 1e4)).collect::<Vec<_>>()));
    // With the sky drawn: neighbouring pixels that both have no surface must match whether or not a
    // tile border lies between them (a lit colour in whole tiles shows as a jump at the borders).
    {
        let (mut sn, mut ss, mut sb, mut inn, mut ins, mut inb) = (0usize, 0f64, 0usize, 0usize, 0f64, 0usize);
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                for vertical in [false, true] {
                    let (a, b) = if vertical { (y * w + x, (y + 1) * w + x) } else { (y * w + x, y * w + x + 1) };
                    if depth[a] != 0.0 || depth[b] != 0.0 {
                        continue;
                    }
                    let (la, lb) = (lum(&px[a]).max(1e-4), lum(&px[b]).max(1e-4));
                    let r = (la / lb).max(lb / la);
                    if (if vertical { y } else { x }) % 8 == 7 {
                        sn += 1;
                        ss += f64::from(r.ln());
                        sb += usize::from(r > 1.05);
                    } else {
                        inn += 1;
                        ins += f64::from(r.ln());
                        inb += usize::from(r > 1.05);
                    }
                }
            }
        }
        out.push(format!(
            "debug: empty: empty-pixel pairs across a tile border {sn}: mean |ln ratio| {:.5}, {sb} over 1.05x; inside tiles {inn}: mean {:.5}, {inb} over 1.05x",
            ss / sn.max(1) as f64,
            ins / inn.max(1) as f64
        ));
    }
    out.push(format!("debug: empty: depth range of lit surface pixels {lit_d:?}, of black ones {black_d:?}"));
    let mut bs = 0;
    for i in (0..w * h).step_by(97) {
        let on = px[i][0] != 0.0 || px[i][1] != 0.0 || px[i][2] != 0.0;
        if depth[i] != 0.0 && !on && bs < 8 {
            bs += 1;
            out.push(format!("debug: empty: black surface ({}, {}) depth {} normal word {:#010x} z {} perm {:?}", i % w, i / w, depth[i], normal[i], (normal[i] >> 20) & 0x3ff, perm_of.as_ref().map(|p| p[i / w / 8 * tw + i % w / 8])));
        }
    }
    // The first lit-but-empty pixels with their G-buffer words.
    let mut shown = 0;
    for i in 0..w * h {
        if depth[i] == 0.0 && (px[i][0] != 0.0 || px[i][1] != 0.0 || px[i][2] != 0.0) && shown < 6 {
            shown += 1;
            out.push(format!("debug: empty: ({}, {}) lit {:?} normal word {:#010x}", i % w, i / w, px[i], normal[i]));
        }
    }
    if let Ok(path) = std::env::var("BO2MP_T7_DEBUG_PNG") {
        if let Ok(f) = std::fs::File::create(&path) {
            let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            let _ = e.write_header().and_then(|mut wr| wr.write_image_data(&img));
        }
    }
    out
}
