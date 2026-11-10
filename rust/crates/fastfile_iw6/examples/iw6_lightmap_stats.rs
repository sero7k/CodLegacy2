//! What a Ghosts map's baked light holds: for each lightmap pair, the two
//! stacked secondary pages' luminance (linear) against the sun's baked
//! visibility (the primary, BC4), in buckets. `iw6_lightmap_stats <mp_map.ff>`.
use fastfile_iw6::map::half_to_f32;

fn main() {
    let path = std::env::args().nth(1).expect("zone");
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let world = map.world.expect("world");
    let mut paks = fastfile_iw6::image::ImagePaks::new(std::path::Path::new("."));
    for (n, &(p, s)) in world.lightmaps.iter().enumerate() {
        let (Some(p), Some(s)) = (p, s) else { continue };
        let pr = &cap.images[p.index as usize];
        let sr = &cap.images[s.index as usize];
        let pp = fastfile_iw6::image::image_pixels(pr, &zone.image, &mut paks, 1 << 14).unwrap();
        let sp = fastfile_iw6::image::image_pixels(sr, &zone.image, &mut paks, 1 << 14).unwrap();
        let (sw, sh) = (sp.width as usize, sp.height as usize);
        let (pw, ph) = (pp.width as usize, pp.height as usize);
        let mut full = vec![0u8; pw * ph];
        for by in 0..ph / 4 {
            for bx in 0..pw / 4 {
                let b = (by * (pw / 4) + bx) * 8;
                bcdec_rs::bc4(&pp.data[b..b + 8], &mut full[(by * 4) * pw + bx * 4..], pw, pp.dxgi_format == 81);
            }
        }
        let page_h = sh / 2;
        let (fx, fy) = (pw / sw, ph / page_h);
        println!("pair {n}: primary {pw}x{ph}, secondary {sw}x{sh}, ratio {fx}x{fy}");
        // buckets of vis: 0..25, 25..100, 100..180, 180..230, 230..256
        let edges = [0u32, 25, 100, 180, 230, 256];
        for page in 0..2 {
            let mut acc = vec![(0f64, 0f64, 0f64, 0f64, 0u64); 5];
            let mut lit_lums: Vec<f32> = Vec::new();
            for y in 0..page_h {
                for x in 0..sw {
                    let i = (page * page_h + y) * sw + x;
                    let h = |c: usize| half_to_f32(u16::from_le_bytes([sp.data[i * 8 + 2 * c], sp.data[i * 8 + 2 * c + 1]]));
                    let (r, g, b, a) = (h(0), h(1), h(2), h(3));
                    let l = f64::from(0.3 * r + 0.59 * g + 0.11 * b);
                    if !l.is_finite() || l <= 0.0 {
                        continue;
                    }
                    let mut sum = 0u32;
                    for dy in 0..fy {
                        for dx in 0..fx {
                            sum += u32::from(full[(y * fy + dy) * pw + x * fx + dx]);
                        }
                    }
                    let v = sum / (fx * fy) as u32;
                    if v >= 230 { lit_lums.push(l as f32); }
                    let k = (0..5).find(|&k| v >= edges[k] && v < edges[k + 1]).unwrap();
                    let e = &mut acc[k];
                    e.0 += l;
                    e.1 += f64::from(r);
                    e.2 += f64::from(b);
                    e.3 += f64::from(a);
                    e.4 += 1;
                }
            }
            lit_lums.sort_by(|a, b| a.total_cmp(b));
            if !lit_lums.is_empty() {
                let q = |f: f32| lit_lums[((lit_lums.len() - 1) as f32 * f) as usize];
                println!("  page {page} sunlit (vis>=230) luminance p10 {:.3} p50 {:.3} p90 {:.3} p99 {:.3} max {:.3}", q(0.1), q(0.5), q(0.9), q(0.99), q(1.0));
            }
            for k in 0..5 {
                let e = acc[k];
                let n = e.4.max(1) as f64;
                println!("  page {page} vis {:>3}..{:<3}: {:>8} texels, lum mean {:.4}, r {:.4} b {:.4}, alpha {:.3}", edges[k], edges[k + 1], e.4, e.0 / n, e.1 / n, e.2 / n, e.3 / n);
            }
        }
    }
    if let Some(s) = map.sun {
        println!("sun dir {:?} colour {:?}", s.dir, s.color);
    }
}
