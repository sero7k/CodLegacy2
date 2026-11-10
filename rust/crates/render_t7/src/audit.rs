//! Debug aids for the native renderer, all off unless asked for by the
//! environment (nothing here runs in the game):
//!
//! * `T7_POISON=<list>` (`all`, or any of `storage`, `uniform`, `tex`): every
//!   buffer the renderer makes without contents starts as the poison word
//!   [`POISON`] (a quiet NaN as a float, a huge index as an integer; storage
//!   buffers read by a shader, constant buffers, never the indirect
//!   arguments, which a dispatch would take as a count) and every texture it
//!   makes without contents starts magenta (float and 8-bit colour formats),
//!   `0xDEADBEEF` in unsigned integer formats. A frame that leaves any of it
//!   in the picture shows an area nothing wrote or a read past what was
//!   written. [`scan`] counts what is left, per resource.
//! * `T7_AUDIT=1` (also set by `T7_POISON`): every translated compute shader
//!   counts its structured/raw/atomic buffer accesses that fall outside the
//!   bound buffer (a real GPU reads zero and drops the write, silently): see
//!   [`oob_report`].
//! * `T7_LIMITS=default`: the offline device gets wgpu's default limits
//!   (what the game's device has: bevy asks for the defaults, raised only
//!   for resolution and alignment) instead of the adapter's.

use std::sync::{Mutex, OnceLock};

use render_h1::gpu::Gpu;

/// The poison word.
pub const POISON: u32 = 0x7FC0_DEAD;

fn flags() -> &'static (bool, bool, bool, bool) {
    static F: OnceLock<(bool, bool, bool, bool)> = OnceLock::new();
    F.get_or_init(|| {
        let v = std::env::var("T7_POISON").unwrap_or_default().to_ascii_lowercase();
        let all = v == "1" || v == "all" || v == "true";
        let has = |k: &str| all || v.split(',').any(|x| x.trim() == k);
        (has("storage"), has("uniform"), has("tex"), std::env::var("T7_AUDIT").is_ok_and(|v| v != "0") || !v.is_empty())
    })
}

pub fn poison_storage() -> bool {
    flags().0
}
pub fn poison_uniform() -> bool {
    flags().1
}
pub fn poison_textures() -> bool {
    flags().2
}
/// The compute shaders count their out-of-bounds buffer accesses.
pub fn bounds_audit() -> bool {
    flags().3
}

struct Registry {
    textures: Vec<(String, wgpu::Texture)>,
    buffers: Vec<(String, wgpu::Buffer)>,
    /// (program, first counter word) of each audited compute program.
    programs: Vec<(String, u32)>,
    oob: Option<wgpu::Buffer>,
}

fn reg() -> &'static Mutex<Registry> {
    static R: OnceLock<Mutex<Registry>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Registry { textures: Vec::new(), buffers: Vec::new(), programs: Vec::new(), oob: None }))
}

/// Counter words per audited program: 64 registers x (t, u) x 3 kinds.
pub const OOB_WORDS: u32 = 64 * 2 * 3;
const OOB_PROGRAMS: u32 = 512;

/// A buffer without contents (zero unless poisoned).
pub fn buffer(device: &wgpu::Device, d: &wgpu::BufferDescriptor) -> wgpu::Buffer {
    let storage = d.usage.contains(wgpu::BufferUsages::STORAGE) && !d.usage.contains(wgpu::BufferUsages::INDIRECT);
    let uniform = d.usage.contains(wgpu::BufferUsages::UNIFORM);
    // Zero by design: stand-ins for what the exe binds as empty.
    let by_design = matches!(d.label, Some("gSunShadowTree" | "t7 forward empty list" | "t7 code instances"));
    let want = !by_design && ((storage && poison_storage()) || (uniform && poison_uniform()));
    if !want || d.usage.contains(wgpu::BufferUsages::MAP_READ) || d.size % 4 != 0 || d.mapped_at_creation {
        let b = device.create_buffer(d);
        if poison_storage() || poison_uniform() || bounds_audit() {
            reg().lock().unwrap().buffers.push((d.label.unwrap_or("?").to_owned(), b.clone()));
        }
        return b;
    }
    let b = device.create_buffer(&wgpu::BufferDescriptor { label: d.label, size: d.size, usage: d.usage | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: true });
    {
        let mut v = b.slice(..).get_mapped_range_mut();
        let fill: Vec<u8> = POISON.to_le_bytes().iter().copied().cycle().take(d.size as usize).collect();
        v.copy_from_slice(&fill);
    }
    b.unmap();
    reg().lock().unwrap().buffers.push((d.label.unwrap_or("?").to_owned(), b.clone()));
    b
}

fn is_depth(f: wgpu::TextureFormat) -> bool {
    f.has_depth_aspect() || f.has_stencil_aspect()
}

fn is_uint(f: wgpu::TextureFormat) -> bool {
    use wgpu::TextureSampleType as S;
    matches!(f.sample_type(None, None), Some(S::Uint | S::Sint))
}

/// A texture without contents (zero unless poisoned: magenta).
pub fn texture(gpu: &Gpu, d: &wgpu::TextureDescriptor) -> wgpu::Texture {
    if !poison_textures() && !bounds_audit() {
        return gpu.device.create_texture(d);
    }
    let colour = !is_depth(d.format) && !d.format.is_compressed();
    let ra = d.usage.contains(wgpu::TextureUsages::RENDER_ATTACHMENT);
    let poison = poison_textures() && colour && ra && d.sample_count == 1;
    let by_write = poison_textures() && colour && !ra && d.sample_count == 1;
    let mut usage = d.usage | wgpu::TextureUsages::COPY_SRC;
    if poison_textures() && colour && !ra {
        usage |= wgpu::TextureUsages::COPY_DST;
    }
    let t = gpu.device.create_texture(&wgpu::TextureDescriptor { label: d.label, size: d.size, mip_level_count: d.mip_level_count, sample_count: d.sample_count, dimension: d.dimension, format: d.format, usage, view_formats: d.view_formats });
    if poison {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let c = if is_uint(d.format) { wgpu::Color { r: f64::from(0xDEAD_BEEFu32), g: f64::from(0xDEAD_BEEFu32), b: f64::from(0xDEAD_BEEFu32), a: f64::from(0xDEAD_BEEFu32) } } else { wgpu::Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 } };
        let layers = if d.dimension == wgpu::TextureDimension::D3 { d.size.depth_or_array_layers } else { d.size.depth_or_array_layers.max(1) };
        for mip in 0..d.mip_level_count {
            for layer in 0..layers {
                let v = t.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(if d.dimension == wgpu::TextureDimension::D3 { wgpu::TextureViewDimension::D3 } else { wgpu::TextureViewDimension::D2 }),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    base_array_layer: if d.dimension == wgpu::TextureDimension::D3 { 0 } else { layer },
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                if d.dimension == wgpu::TextureDimension::D3 && layer > 0 {
                    break;
                }
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("t7 poison"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &v, depth_slice: if d.dimension == wgpu::TextureDimension::D3 { Some(0) } else { None }, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(c), store: wgpu::StoreOp::Store } })],
                    ..Default::default()
                });
            }
        }
        gpu.queue.submit([enc.finish()]);
    }
    if by_write {
        if let Some(px) = pattern(d.format) {
            let (bw, bh) = d.format.block_dimensions();
            let _ = bh;
            let _ = bw;
            let layers = d.size.depth_or_array_layers.max(1);
            for mip in 0..d.mip_level_count {
                let (w, h) = ((d.size.width >> mip).max(1), (d.size.height >> mip).max(1));
                let depth = if d.dimension == wgpu::TextureDimension::D3 { (layers >> mip).max(1) } else { layers };
                let row = px.len() * w as usize;
                let data: Vec<u8> = px.iter().copied().cycle().take(row * h as usize).collect();
                for z in 0..depth {
                    gpu.queue.write_texture(
                        wgpu::TexelCopyTextureInfo { texture: &t, mip_level: mip, origin: wgpu::Origin3d { x: 0, y: 0, z }, aspect: wgpu::TextureAspect::All },
                        &data,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row as u32), rows_per_image: Some(h) },
                        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    );
                }
            }
        }
    }
    reg().lock().unwrap().textures.push((d.label.unwrap_or("?").to_owned(), t.clone()));
    t
}

/// One texel of the poison, in the formats the renderer writes.
fn pattern(f: wgpu::TextureFormat) -> Option<Vec<u8>> {
    use wgpu::TextureFormat as F;
    let words = |v: &[u32]| v.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
    let halves = |v: &[u16]| v.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
    Some(match f {
        F::Rgba8Unorm | F::Rgba8UnormSrgb | F::Bgra8Unorm | F::Bgra8UnormSrgb => vec![255, 0, 255, 255],
        F::Rgba16Float => halves(&[0x3C00, 0, 0x3C00, 0x3C00]),
        F::Rgba32Float => words(&[0x3F80_0000, 0, 0x3F80_0000, 0x3F80_0000]),
        F::R32Float => words(&[0x3F80_0000]),
        F::R32Uint | F::R32Sint => words(&[0xDEAD_BEEF]),
        F::Rg32Float => words(&[0x3F80_0000, 0]),
        F::Rgba32Uint => words(&[0xDEAD_BEEF; 4]),
        F::Rg16Float => halves(&[0x3C00, 0]),
        F::R16Float => halves(&[0x3C00]),
        F::R8Unorm => vec![255],
        F::Rg8Unorm => vec![255, 0],
        F::Rgb10a2Unorm => words(&[3 << 30 | 0x3FF << 20 | 0x3FF]),
        _ => return None,
    })
}

/// The counters of the audited compute programs: made once, bound by every
/// audited program (`T7_AUDIT`).
pub fn oob_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    let mut r = reg().lock().unwrap();
    r.oob.get_or_insert_with(|| device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7_oob"), size: u64::from(OOB_WORDS * OOB_PROGRAMS * 4), usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })).clone()
}

/// The first counter word of a program, new for each name.
pub fn oob_base(name: &str) -> u32 {
    let mut r = reg().lock().unwrap();
    if let Some((_, b)) = r.programs.iter().find(|(n, _)| n == name) {
        return *b;
    }
    let base = (r.programs.len() as u32 % OOB_PROGRAMS) * OOB_WORDS;
    r.programs.push((name.to_owned(), base));
    base
}

fn read_buffer(gpu: &Gpu, b: &wgpu::Buffer, offset: u64, size: u64) -> Vec<u8> {
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(b, offset, &rb, 0, size);
    gpu.queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    rb.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    if rx.recv().is_ok_and(|r| r.is_ok()) {
        let v = rb.slice(..).get_mapped_range().to_vec();
        v
    } else {
        Vec::new()
    }
}

/// The out-of-bounds accesses counted since the last call, one line per
/// program and buffer register, and the counters cleared.
pub fn oob_report(gpu: &Gpu) -> Vec<String> {
    let (buf, programs) = {
        let r = reg().lock().unwrap();
        (r.oob.clone(), r.programs.clone())
    };
    let Some(buf) = buf else { return Vec::new() };
    let bytes = read_buffer(gpu, &buf, 0, u64::from(OOB_WORDS * OOB_PROGRAMS * 4));
    let mut out = Vec::new();
    for (name, base) in programs {
        for w in 0..OOB_WORDS {
            let at = ((base + w) * 4) as usize;
            let Some(c) = bytes.get(at..at + 4) else { continue };
            let n = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            if n > 0 {
                let (reg, kind) = ((w / 3) % 64, ["read", "write", "atomic"][(w % 3) as usize]);
                let k = if w / 3 >= 64 { 'u' } else { 't' };
                out.push(format!("OOB {name}: {k}{reg} {kind}: {n} words outside the bound buffer"));
            }
        }
    }
    gpu.queue.write_buffer(&buf, 0, &vec![0u8; (OOB_WORDS * OOB_PROGRAMS * 4) as usize]);
    out
}

/// The newest of each label (a resize or a re-bind makes new ones).
fn latest<T>(list: Vec<(String, T)>) -> Vec<(String, T)> {
    let mut out: Vec<(String, T)> = Vec::new();
    for (l, t) in list {
        if let Some(e) = out.iter_mut().find(|(n, _)| *n == l) {
            e.1 = t;
        } else {
            out.push((l, t));
        }
    }
    out
}

fn texel_bytes(f: wgpu::TextureFormat) -> Option<u32> {
    f.block_copy_size(None)
}

/// Does this texel (raw bytes) still hold what the poison wrote?
fn poisoned(f: wgpu::TextureFormat, t: &[u8]) -> bool {
    use wgpu::TextureFormat as F;
    let h = |i: usize| u16::from_le_bytes([t[2 * i], t[2 * i + 1]]);
    let w = |i: usize| u32::from_le_bytes([t[4 * i], t[4 * i + 1], t[4 * i + 2], t[4 * i + 3]]);
    match f {
        // Magenta is the same bytes in either channel order.
        F::Rgba8Unorm | F::Rgba8UnormSrgb | F::Bgra8Unorm | F::Bgra8UnormSrgb => t == [255, 0, 255, 255],
        F::Rgba16Float => h(0) == 0x3C00 && h(1) == 0 && h(2) == 0x3C00 && h(3) == 0x3C00,
        F::Rgba32Float => w(0) == 0x3F80_0000 && w(1) == 0 && w(2) == 0x3F80_0000 && w(3) == 0x3F80_0000,
        F::R32Float | F::R32Uint | F::R32Sint => matches!(w(0), 0x3F80_0000 | 0xDEAD_BEEF),
        F::Rg16Float => h(0) == 0x3C00 && h(1) == 0,
        F::Rg32Float => w(0) == 0x3F80_0000 && w(1) == 0,
        F::R16Float => h(0) == 0x3C00,
        F::R8Unorm => t[0] == 255,
        F::Rg8Unorm => t == [255, 0],
        F::Rgb10a2Unorm => w(0) == (3 << 30 | 0x3FF << 20 | 0x3FF),
        F::Rgba32Uint => w(0) == 0xDEAD_BEEF,
        _ => false,
    }
}

/// Reads back every registered texture (mip 0) and counts the texels still
/// poisoned, with the bounding box of them: the areas nothing wrote.
/// `only`: labels containing it (`None`: all).
pub fn scan(gpu: &Gpu, only: Option<&str>) -> Vec<String> {
    let list = latest(reg().lock().unwrap().textures.clone());
    let mut out = Vec::new();
    for (label, t) in list {
        if only.is_some_and(|o| !label.contains(o)) {
            continue;
        }
        let f = t.format();
        let (Some(bpp), true) = (texel_bytes(f), t.usage().contains(wgpu::TextureUsages::COPY_SRC)) else { continue };
        if is_depth(f) || f.is_compressed() || t.dimension() != wgpu::TextureDimension::D2 || t.sample_count() != 1 {
            continue;
        }
        for mip in 0..t.mip_level_count() {
        let label = if t.mip_level_count() > 1 { format!("{label}[mip {mip}]") } else { label.clone() };
        let (w, h) = ((t.width() >> mip).max(1), (t.height() >> mip).max(1));
        let layers = t.depth_or_array_layers();
        let row = (w * bpp).div_ceil(256) * 256;
        if u64::from(row) * u64::from(h) * u64::from(layers) > 600 << 20 {
            out.push(format!("poison scan: {label} {w}x{h}x{layers} {f:?}: too big to read back"));
            continue;
        }
        let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row) * u64::from(h) * u64::from(layers), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers },
        );
        gpu.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        rb.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        if !rx.recv().is_ok_and(|r| r.is_ok()) {
            continue;
        }
        let data = rb.slice(..).get_mapped_range();
        let (mut n, mut nan, mut bb) = (0u64, 0u64, [u32::MAX, u32::MAX, 0, 0]);
        for l in 0..layers {
            for y in 0..h {
                let base = (u64::from(l) * u64::from(h) + u64::from(y)) as usize * row as usize;
                for x in 0..w {
                    let o = base + (x * bpp) as usize;
                    let px = &data[o..o + bpp as usize];
                    if poisoned(f, px) {
                        n += 1;
                        bb = [bb[0].min(x), bb[1].min(y), bb[2].max(x), bb[3].max(y)];
                    } else if matches!(f, wgpu::TextureFormat::Rgba16Float) && (0..4).any(|i| (u16::from_le_bytes([px[2 * i], px[2 * i + 1]]) & 0x7C00) == 0x7C00) {
                        nan += 1;
                    } else if matches!(f, wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::R32Float) && (0..bpp as usize / 4).any(|i| f32::from_le_bytes([px[4 * i], px[4 * i + 1], px[4 * i + 2], px[4 * i + 3]]).is_nan()) {
                        nan += 1;
                    }
                }
            }
        }
        let total = u64::from(w) * u64::from(h) * u64::from(layers);
        if n > 0 || nan > 0 {
            out.push(format!(
                "poison scan: {label} {w}x{h}x{layers} {f:?}: {n} of {total} texels unwritten{}, {nan} NaN/Inf",
                if n > 0 { format!(" (box x {}..{} y {}..{})", bb[0], bb[2], bb[1], bb[3]) } else { String::new() }
            ));
        } else {
            out.push(format!("poison scan: {label} {w}x{h}x{layers} {f:?}: clean"));
        }
        }
    }
    out
}

/// Reads back every registered buffer and reports the poison words left:
/// how many, and the first and last (a byte offset; the part of the buffer
/// nothing wrote).
pub fn scan_buffers(gpu: &Gpu) -> Vec<String> {
    let list = latest(reg().lock().unwrap().buffers.clone());
    let mut out = Vec::new();
    for (label, b) in list {
        if !b.usage().contains(wgpu::BufferUsages::COPY_SRC) || b.size() > 600 << 20 || label == "t7_oob" {
            continue;
        }
        let bytes = read_buffer(gpu, &b, 0, b.size());
        let (mut n, mut first, mut last) = (0u64, u64::MAX, 0u64);
        for (i, w) in bytes.chunks_exact(4).enumerate() {
            if u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == POISON {
                n += 1;
                first = first.min(i as u64 * 4);
                last = last.max(i as u64 * 4);
            }
        }
        let words = b.size() / 4;
        if n > 0 {
            out.push(format!("poison scan: buffer {label} ({} bytes): {n} of {words} words never written (bytes {first}..{})", b.size(), last + 4));
        } else {
            out.push(format!("poison scan: buffer {label} ({} bytes): fully written", b.size()));
        }
    }
    out
}
