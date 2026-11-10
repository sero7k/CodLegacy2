//! A zone's colour lookup tables (CLUT assets): name, size and the mean
//! colour change they make (output minus input over the cube).
//!
//! `cargo run -p fastfile_h1 --release --example h1_cluts -- <zone.ff>`
fn main() {
    let p = std::env::args().nth(1).expect("zone");
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for (name, dims, span) in &cap.cluts {
        let Some(s) = span else {
            println!("{name} {dims:?} (no pixels)");
            continue;
        };
        let px = s.get(&z.image);
        let [w, h, d] = dims.map(|v| v as usize);
        let (mut sum_in, mut sum_out, mut n) = ([0f64; 3], [0f64; 3], 0f64);
        for b in 0..d {
            for g in 0..h {
                for r in 0..w {
                    let i = ((b * h + g) * w + r) * 4;
                    let input = [r as f64 / (w - 1) as f64, g as f64 / (h - 1) as f64, b as f64 / (d - 1) as f64];
                    for c in 0..3 {
                        sum_in[c] += input[c];
                        sum_out[c] += f64::from(px[i + c]) / 255.0;
                    }
                    n += 1.0;
                }
            }
        }
        let mid = ((d / 2 * h + h / 2) * w + w / 2) * 4;
        println!(
            "{name} {w}x{h}x{d}: mean in {:.3?} out {:.3?}; mid grey -> {:?} (first texel {:?}, last {:?})",
            sum_in.map(|v| v / n),
            sum_out.map(|v| v / n),
            &px[mid..mid + 4],
            &px[0..4],
            &px[px.len() - 4..]
        );
    }
}
