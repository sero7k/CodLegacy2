//! First-level byte statistics of a picture as the renderer reads it.
//!
//! `cargo run -p render_h1 --example h1_image_stats -- <image> <map.ff> <zone.ff>...`

use render_h1::scene::Scene;
use render_h1::world::ImageSource;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let zones: Vec<std::path::PathBuf> = a[1..].iter().map(Into::into).collect();
    let mut scene = Scene::load(&zones).expect("load");
    let (px, faces) = scene.images.image(&a[0]).expect("image");
    println!(
        "{}x{} levels {} dxgi {} faces {faces} bytes {}",
        px.width,
        px.height,
        px.levels,
        px.dxgi_format,
        px.data.len()
    );
    let zero = px.data.iter().filter(|&&b| b == 0).count();
    println!(
        "zero bytes {:.1}% (first 16: {:02x?})",
        100.0 * zero as f64 / px.data.len().max(1) as f64,
        &px.data[..px.data.len().min(16)]
    );
    let n = (px.width * px.height) as usize;
    if px.dxgi_format == 61 {
        let d = &px.data[..n.min(px.data.len())];
        let mut hist = [0usize; 8];
        for &b in d {
            hist[usize::from(b) / 32] += 1;
        }
        println!(
            "mean {:.3} histogram/32 {hist:?}",
            d.iter().map(|&b| f64::from(b)).sum::<f64>() / d.len() as f64 / 255.0
        );
    }
    // BC1/BC2/BC3: the average of the colour blocks' two endpoints (first
    // level), as 0..1 (no sRGB decode).
    if matches!(px.dxgi_format, 71 | 72 | 74 | 75 | 77 | 78) {
        let (stride, colour_at) = match px.dxgi_format {
            71 | 72 => (8, 0),
            _ => (16, 8),
        };
        let blocks = (px.width.div_ceil(4) * px.height.div_ceil(4)) as usize;
        let mut sum = [0f64; 3];
        let rgb = |v: u16| {
            [
                f64::from((v >> 11) & 31) / 31.0,
                f64::from((v >> 5) & 63) / 63.0,
                f64::from(v & 31) / 31.0,
            ]
        };
        for b in 0..blocks {
            let at = b * stride + colour_at;
            for e in 0..2 {
                let v = u16::from_le_bytes([px.data[at + 2 * e], px.data[at + 2 * e + 1]]);
                let c = rgb(v);
                for i in 0..3 {
                    sum[i] += c[i];
                }
            }
        }
        let n = (blocks * 2) as f64;
        println!(
            "mean colour (endpoints) {:.3} {:.3} {:.3}",
            sum[0] / n,
            sum[1] / n,
            sum[2] / n
        );
    }
    // RGBA16F: per-channel percentiles, top and bottom halves apart (a
    // secondary lightmap page holds two pictures stacked).
    if px.dxgi_format == 10 {
        let half = |h: u16| -> f32 {
            let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 31) as i32, (h & 1023) as f32);
            let v = match e {
                0 => m * 2f32.powi(-24),
                31 => f32::INFINITY,
                _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
            };
            if s == 1 { -v } else { v }
        };
        let rows = px.height as usize;
        for (label, r0, r1) in [("top", 0, rows / 2), ("bottom", rows / 2, rows)] {
            let mut ch: [Vec<f32>; 4] = Default::default();
            for y in (r0..r1).step_by(4) {
                for x in (0..px.width as usize).step_by(4) {
                    let at = (y * px.width as usize + x) * 8;
                    for (c, v) in ch.iter_mut().enumerate() {
                        let b = at + 2 * c;
                        v.push(half(u16::from_le_bytes([px.data[b], px.data[b + 1]])));
                    }
                }
            }
            for v in &mut ch {
                v.sort_by(f32::total_cmp);
            }
            let p = |v: &Vec<f32>, q: f32| v[((v.len() - 1) as f32 * q) as usize];
            for (c, v) in ch.iter().enumerate() {
                println!(
                    "{label} ch{c}: min {:.4} p10 {:.4} p50 {:.4} p90 {:.4} max {:.4}",
                    p(v, 0.0),
                    p(v, 0.1),
                    p(v, 0.5),
                    p(v, 0.9),
                    p(v, 1.0)
                );
            }
        }
    }
}
