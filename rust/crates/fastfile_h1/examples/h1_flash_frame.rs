//! A first-person frame of a Modern Warfare Remastered / Horizon MW gun's
//! first shot, drawn on the CPU the way the runtime's effect pass draws it
//! (`render_gpu` `EFFECT`: gamma-space texel times the element's colour
//! bytes, added or blended), from the gun's own `viewFlashEffect`, to check
//! its pixels offline. The element colours go through the same conversion as
//! `asset_game::fx_catalog_s1::s1_vis_sample_to_iw4` (`--old`: the colour
//! written as it is stored, linear; default: encoded to sRGB, as the bytes
//! are read). Prints the frame's mean added colour and red excess, and
//! writes a PNG.
//!
//! `cargo run -p fastfile_h1 --example h1_flash_frame -- [--old] [--cap] --gun <name>
//! --out <frame.png> [--ms 16] <zone.ff>...`

use std::path::PathBuf;

use fastfile_h1::layout::{FxElemDef as E, FxElemVisStateSample as V, WeaponDef as D};
use fastfile_h1::walk::{Capture, FxRec, walk_zone};

struct Z {
    path: PathBuf,
    image: Vec<u8>,
    cap: Capture,
}

const W: usize = 480;
const H: usize = 270;
const BG: f32 = 0.30;

fn f32_at(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4)
        .map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4)
        .map_or(0, |s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// One sample's colour (base, amplitude) as the runtime's bytes / 255.
fn converted(s: &[u8], encode: bool) -> [[f32; 4]; 2] {
    let lit = |c: usize, e: usize| {
        let mut v = [f32_at(s, c), f32_at(s, c + 4), f32_at(s, c + 8), f32_at(s, c + 12)];
        for k in 0..3 {
            let sc = f32_at(s, e + 4 * k);
            if sc.is_finite() && sc > 0.0 {
                v[k] *= sc;
            }
        }
        v
    };
    let base = lit(V::base__color, V::base__emissiveScale);
    let amp = lit(V::amplitude__color, V::amplitude__emissiveScale);
    let peak = if encode {
        (0..3).map(|k| base[k].max(amp[k])).fold(0.0f32, f32::max)
    } else {
        (0..3).map(|k| base[k] + amp[k].max(0.0)).fold(0.0f32, f32::max)
    };
    let squeeze = if peak > 1.0 { 1.0 / peak } else { 1.0 };
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() / 255.0;
    [base, amp].map(|c| {
        let ch = |k: usize| {
            let v = c[k] * squeeze;
            q(if encode { srgb(v) } else { v })
        };
        [ch(0), ch(1), ch(2), q(c[3])]
    })
}

struct Tex {
    w: usize,
    h: usize,
    /// Gamma-space RGBA (an sRGB picture read through its view and
    /// re-encoded, as `raw_texel` does).
    px: Vec<[f32; 4]>,
}

fn texture(zones: &[Z], cap: &Capture, material: &str) -> Option<(String, Tex)> {
    let m = zones
        .iter()
        .flat_map(|z| z.cap.materials.iter().map(move |m| (z, m)))
        .find(|(_, m)| !m.name.starts_with(',') && m.name == material)
        .map(|(z, m)| (&z.cap, m))
        .or_else(|| cap.materials.iter().find(|m| m.name == material).map(|m| (cap, m)))?;
    let (mcap, m) = m;
    let tech = m.techset.map(|t| mcap.name_of(t).trim_start_matches(',').to_owned()).unwrap_or_default();
    if tech.starts_with("distortion") || tech.contains("radialblur") || tech.contains("dustonlens") {
        return None;
    }
    let img = m
        .textures
        .iter()
        .find(|t| t.name_hash == 0xa0ab_1041)
        .or_else(|| m.textures.iter().find(|t| t.semantic == 2))?
        .image
        .map(|i| mcap.name_of(i).trim_start_matches(',').to_owned())?;
    let (z, rec) = zones.iter().find_map(|z| {
        z.cap
            .images
            .iter()
            .find(|r| r.name.trim_start_matches(',') == img && !fastfile_h1::shared::is_reference(r))
            .map(|r| (z, r))
    })?;
    let retail = std::env::var_os("H1_IMAGEFILES").map(PathBuf::from);
    let mut paks = fastfile_h1::image::ImagePaks::for_zone(&z.path, retail.as_deref());
    let px = fastfile_h1::image::image_pixels(rec, &z.image, &mut paks, 256)
        .map_err(|e| eprintln!("{img}: {e}"))
        .ok()?;
    let (w, h) = (px.width as usize, px.height as usize);
    let out: Vec<[f32; 4]> = if matches!(px.dxgi_format, 95 | 96) {
        let mut out = vec![[0f32; 4]; w * h];
        let mut block = [0f32; 48];
        for by in 0..h / 4 {
            for bx in 0..w / 4 {
                let b = &px.data[(by * (w / 4) + bx) * 16..][..16];
                bcdec_rs::bc6h_float(b, &mut block, 12, px.dxgi_format == 96);
                for y in 0..4 {
                    for x in 0..4 {
                        let t = &block[y * 12 + x * 3..][..3];
                        out[(by * 4 + y) * w + bx * 4 + x] = [srgb(t[0]), srgb(t[1]), srgb(t[2]), 1.0];
                    }
                }
            }
        }
        out
    } else {
        let (_, _, b) = fastfile_h1::image::rgba8_level0(&px)
            .map_err(|e| eprintln!("{img}: {e}"))
            .ok()?;
        b.chunks_exact(4)
            .map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0])
            .collect()
    };
    Some((format!("{material} ({tech}, {img})"), Tex { w, h, px: out }))
}

fn find_fx<'a>(zones: &'a [Z], name: &str) -> Option<(&'a Z, &'a FxRec)> {
    zones.iter().find_map(|z| {
        z.cap
            .fx
            .iter()
            .find(|f| !f.name.starts_with(',') && f.name == name)
            .map(|f| (z, f))
    })
}

struct Frame {
    rgb: Vec<[f32; 3]>,
    encode: bool,
    ms: f32,
    /// `--h1`: the reference, blended in linear light as the game's own
    /// renderer does (colours linear, pictures decoded from sRGB), shown
    /// sRGB-encoded at the end.
    h1: bool,
}

fn lin(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

/// Draws `name`'s elements alive at `ms` (spawned at `delay`) at `origin`
/// (inches ahead of the eye, then right, then up).
fn draw_fx(frame: &mut Frame, zones: &[Z], name: &str, origin: [f32; 3], delay: f32, depth: usize) {
    let Some((z, f)) = find_fx(zones, name) else {
        return;
    };
    for el in &f.elems {
        let r = el.record.get(&z.image);
        let spawn_delay = delay + i32_at(r, E::spawnDelayMsec) as f32;
        let ahead = f32_at(r, E::spawnOrigin) + 0.5 * f32_at(r, E::spawnOrigin + 4);
        let at = [origin[0] + ahead, origin[1], origin[2]];
        if el.elem_type == 16 {
            if depth < 4 && spawn_delay <= frame.ms {
                // (A runner plays one of its effects, picked at random: the first.)
                if let Some(c) = el.effects.first() {
                    draw_fx(frame, zones, c.trim_start_matches(','), at, spawn_delay, depth + 1);
                }
            }
            continue;
        }
        // Sprites only (billboards, oriented, clouds as sprites).
        if !matches!(el.elem_type, 0 | 1 | 2 | 8) {
            continue;
        }
        let life = i32_at(r, E::lifeSpanMsec) as f32 + 0.5 * i32_at(r, E::lifeSpanMsec + 4) as f32;
        let age = frame.ms - spawn_delay;
        if life <= 0.0 || age < 0.0 || age > life {
            continue;
        }
        let Some(mat) = el.materials.first() else { continue };
        let mname = z.cap.name_of(*mat).trim_start_matches(',').to_owned();
        let Some((label, tex)) = texture(zones, &z.cap, &mname) else {
            continue;
        };
        let vis = el.vis_samples.map(|s| s.get(&z.image).to_vec()).unwrap_or_default();
        let samples: Vec<&[u8]> = vis.chunks_exact(V::SIZE).collect();
        if samples.is_empty() {
            continue;
        }
        let intervals = r[E::visStateIntervalCount] as usize;
        let t = (age / life).clamp(0.0, 1.0) * intervals as f32;
        let i0 = (t.floor() as usize).min(samples.len() - 1);
        let i1 = (i0 + 1).min(samples.len() - 1);
        let fr = t - t.floor();
        let rand = 0.5;
        let col = |s: &[u8]| {
            let [b, a] = converted(s, frame.encode && !frame.h1);
            [0, 1, 2, 3].map(|k| b[k] * (1.0 - rand) + a[k] * rand)
        };
        let (c0, c1) = (col(samples[i0]), col(samples[i1]));
        let tint: [f32; 4] = [0, 1, 2, 3].map(|k| c0[k] * (1.0 - fr) + c1[k] * fr);
        let size = |s: &[u8]| f32_at(s, V::base__size) + f32_at(s, V::amplitude__size) * rand;
        let mut half = size(samples[i0]) * (1.0 - fr) + size(samples[i1]) * fr;
        // `--cap`: held as the runtime holds it (`fx_iw4::FX_VIEW_FLASH_SPRITE_TAN_MAX`).
        // and a lens overlay left out as the runtime leaves it out
        // (`fx_iw4::fx_is_lens_overlay_material`).
        if std::env::args().any(|a| a == "--cap") {
            half = half.min(0.25 * at[0]);
            let lens = ["dirty_lens", "dustonlens", "lensflare", "flare_line", "lens_flare", "lens_dirt"];
            if lens.iter().any(|m| mname.to_ascii_lowercase().contains(m)) {
                println!("  {} {mname}: lens overlay, not drawn", f.name);
                continue;
            }
        }
        if half <= 0.0 {
            continue;
        }
        let tech = label.clone();
        let add = tech.contains("_add");
        // Atlas cell 0.
        let (cb, rb) = (r[E::atlas + 4] & 0x1f, r[E::atlas + 5] & 0x1f);
        let (cw, ch) = (tex.w >> cb, tex.h >> rb);
        // Project: 65 degree horizontal field of view.
        let focal = (W as f32 / 2.0) / (65f32.to_radians() / 2.0).tan();
        let (cx, cy) = (
            W as f32 / 2.0 + at[1] / at[0] * focal,
            H as f32 / 2.0 - at[2] / at[0] * focal,
        );
        let rad = half / at[0] * focal;
        let mut added = [0f32; 3];
        for y in (cy - rad).max(0.0) as usize..((cy + rad).min(H as f32) as usize) {
            for x in (cx - rad).max(0.0) as usize..((cx + rad).min(W as f32) as usize) {
                let u = (x as f32 - (cx - rad)) / (2.0 * rad);
                let v = (y as f32 - (cy - rad)) / (2.0 * rad);
                let tx = ((u * cw as f32) as usize).min(cw.max(1) - 1);
                let ty = ((v * ch as f32) as usize).min(ch.max(1) - 1);
                let mut texel = tex.px[ty * tex.w + tx];
                if frame.h1 {
                    for t in texel.iter_mut().take(3) {
                        *t = lin(*t);
                    }
                }
                let a = texel[3] * tint[3];
                let p = &mut frame.rgb[y * W + x];
                for k in 0..3 {
                    let c = texel[k] * tint[k];
                    let before = p[k];
                    p[k] = if add { p[k] + c * a } else { p[k] * (1.0 - a) + c * a };
                    added[k] += p[k] - before;
                }
            }
        }
        let n = (W * H) as f32;
        println!(
            "  {name} @{spawn_delay:.0}ms age {age:.0}/{life:.0} {label}: tint [{:.3} {:.3} {:.3} {:.3}] half {half:.1} at {:.0}in, frame delta [{:+.4} {:+.4} {:+.4}]",
            tint[0],
            tint[1],
            tint[2],
            tint[3],
            at[0],
            added[0] / n,
            added[1] / n,
            added[2] / n
        );
    }
}

fn main() {
    let (mut gun, mut out, mut encode, mut ms) = (String::new(), None, true, 16.0f32);
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--gun" => gun = args.next().unwrap_or_default(),
            "--out" => out = args.next().map(PathBuf::from),
            "--old" => encode = false,
            "--h1" | "--cap" => {}
            "--ms" => ms = args.next().and_then(|s| s.parse().ok()).unwrap_or(ms),
            _ => paths.push(PathBuf::from(a)),
        }
    }
    let mut zones = Vec::new();
    for p in &paths {
        match fastfile_h1::Zone::open(p) {
            Ok(z) => {
                let cap = walk_zone(&z).0;
                zones.push(Z { path: p.clone(), image: z.image, cap });
            }
            Err(e) => eprintln!("{}: {e}", p.display()),
        }
    }
    let Some((z, w)) = zones
        .iter()
        .find_map(|z| z.cap.weapons.iter().find(|w| w.name == gun).map(|w| (z, w)))
    else {
        eprintln!("no gun {gun}");
        return;
    };
    let Some(flash) = w
        .fx
        .iter()
        .find(|(o, _)| *o == D::viewFlashEffect)
        .map(|(_, id)| z.cap.name_of(*id).trim_start_matches(',').to_owned())
    else {
        eprintln!("{gun}: no viewFlashEffect");
        return;
    };
    println!("{gun}: viewFlashEffect {flash}, {} colours, frame at {ms} ms", if encode { "sRGB-encoded" } else { "linear (as stored)" });
    let h1 = std::env::args().any(|a| a == "--h1");
    let bg = if h1 { lin(BG) } else { BG };
    let mut frame = Frame { rgb: vec![[bg; 3]; W * H], encode, ms, h1 };
    // The muzzle: 22 inches ahead, a little right of and below the eye.
    draw_fx(&mut frame, &zones, &flash, [22.0, 3.0, -2.5], 0.0, 0);
    if h1 {
        for p in frame.rgb.iter_mut() {
            *p = p.map(srgb);
        }
    }
    let n = (W * H) as f32;
    let mut mean = [0f32; 3];
    let (mut red_px, mut lit_px) = (0usize, 0usize);
    for p in &frame.rgb {
        let d = [p[0] - BG, p[1] - BG, p[2] - BG];
        for k in 0..3 {
            mean[k] += d[k] / n;
        }
        if d.iter().any(|c| c.abs() > 0.02) {
            lit_px += 1;
            // Red-dominant: red clearly above both green and blue.
            if p[0] > 1.35 * p[1].max(0.01) && p[0] > 1.6 * p[2].max(0.01) {
                red_px += 1;
            }
        }
    }
    println!(
        "frame: mean added [{:+.4} {:+.4} {:+.4}], touched {lit_px} px, red-dominant {red_px} px ({:.1}% of touched)",
        mean[0],
        mean[1],
        mean[2],
        100.0 * red_px as f32 / lit_px.max(1) as f32
    );
    if let Some(out) = out {
        let file = std::fs::File::create(&out).expect("create png");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().expect("png header");
        let bytes: Vec<u8> = frame
            .rgb
            .iter()
            .flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect();
        wr.write_image_data(&bytes).expect("png data");
        println!("wrote {}", out.display());
    }
}
