//! Looks for the tonemap's colour grade in decoded zones:
//!
//! `jup_lut_search <zone>...`
//!
//! Prints every picture that is volume (depth over 1) or a strip a lookup
//! table is stored as (width = height squared, up to 64 slices), with its
//! hash, format and usage byte, so the grade's picture can be named.
use fastfile_jup::image::{self, ImageRun};
use fastfile_jup::map::ZoneImage;

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let zone = match ZoneImage::parse(bytes) {
            Ok(z) => z,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let count = zone.types.iter().filter(|&&t| t == 21).count();
        let Some(run) = ImageRun::find(&zone.bytes, count) else {
            println!("{path}: {count} images, no run found");
            continue;
        };
        let (mut volumes, mut strips, mut total) = (0, 0, 0);
        for &at in &run.at {
            let Ok(Some(r)) = image::parse(&zone.bytes, at) else { continue };
            total += 1;
            let (w, h, d) = (u32::from(r.width), u32::from(r.height), u32::from(r.depth));
            let volume = d > 1;
            let strip = d == 1 && h >= 8 && h <= 64 && w == h * h;
            if volume {
                volumes += 1;
            }
            if strip {
                strips += 1;
            }
            if volume && w == 32 && h == 32 && d == 32 && r.format == 0xc {
                if let Some((o, n)) = r.inline {
                    let data = &zone.bytes[o..o + n];
                    let px = |x: usize, y: usize, z: usize| -> [u16; 4] {
                        let k = (x + 32 * y + 1024 * z) * 8;
                        let g = |i: usize| data.get(k + 2 * i..k + 2 * i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                        [g(0), g(1), g(2), g(3)]
                    };
                    let half = |v: u16| {
                        let e = i32::from((v >> 10) & 31);
                        let m = f32::from(v & 1023);
                        let x = if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
                        if v & 0x8000 != 0 { -x } else { x }
                    };
                    let all: Vec<u16> = data.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
                    let (mn, mx) = (all.iter().min().copied().unwrap_or(0), all.iter().max().copied().unwrap_or(0));
                    let mean_unorm = all.iter().map(|&v| f64::from(v)).sum::<f64>() / all.len().max(1) as f64 / 65535.0;
                    println!("   u16 range {mn}..{mx}, mean {mean_unorm:.3} as unorm; half of max {:.3}", half(mx));
                    for q in [0usize, 4, 8, 12, 16, 20, 24, 28, 31] {
                        let t = px(q, q, q);
                        println!("   diag {q:2}: u16 {t:?} half {:.3} {:.3} {:.3} {:.3}", half(t[0]), half(t[1]), half(t[2]), half(t[3]));
                    }
                    // How close each assignment of the memory axes to the colour channels is to identity.
                    for perm in [[0usize, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
                        let mut err = 0.0f64;
                        for z in (0..32).step_by(3) {
                            for y in (0..32).step_by(3) {
                                for x in (0..32).step_by(3) {
                                    let t = px(x, y, z);
                                    let idx = [x, y, z];
                                    for ch in 0..3 {
                                        err += (f64::from(t[ch]) / 65535.0 - idx[perm[ch]] as f64 / 31.0).abs();
                                    }
                                }
                            }
                        }
                        println!("   channels read from axes {perm:?}: mean error {:.4}", err / (11.0 * 11.0 * 11.0 * 3.0));
                    }
                    let (a, b, c) = (px(31, 0, 0), px(0, 31, 0), px(0, 0, 31));
                    println!("   axes: x31 {a:?} y31 {b:?} z31 {c:?}");
                }
            }
            if volume || strip {
                println!("{path}: {:016x} {w}x{h}x{d} array {} format {:#x} usage {:#x} levels {} groups {} inline {}", r.hash, r.array, r.format, r.usage, r.levels, r.groups.len(), r.inline.is_some());
            }
        }
        println!("{path}: {total} images, {volumes} volumes, {strips} strips");
    }
}
