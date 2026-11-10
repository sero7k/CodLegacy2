//! Prints a map zone's lightmap pages (formats, sizes, texel statistics):
//! `cargo run -p fastfile_h1 --release --example h1_lightmap_dump -- <zone.ff>`.

fn main() {
    let path: std::path::PathBuf = std::env::args_os().nth(1).expect("zone path").into();
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (map, cap) = fastfile_h1::map::IwMap::load(&zone);
    let mut paks = fastfile_h1::image::ImagePaks::for_zone(&path, None);
    for (i, &(p, s)) in map
        .world
        .as_ref()
        .expect("world")
        .lightmaps
        .iter()
        .enumerate()
    {
        for (what, id) in [("primary", p), ("secondary", s)] {
            let Some(rec) = id.and_then(|i| cap.images.get(i.index as usize)) else {
                continue;
            };
            match fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, 1 << 14) {
                Ok(px) => {
                    println!(
                        "page {i} {what} {}: {}x{} format {} levels {} bytes {}",
                        rec.name,
                        px.width,
                        px.height,
                        px.dxgi_format,
                        px.levels,
                        px.data.len()
                    );
                    if let Some(d) = std::env::args_os().nth(2) {
                        let f = std::path::Path::new(&d).join(format!("page{i}_{what}_{}x{}_f{}.raw", px.width, px.height, px.dxgi_format));
                        std::fs::write(f, &px.data).expect("write");
                    }
                    if px.dxgi_format == 28 || px.dxgi_format == 27 {
                        let n = (px.width * px.height) as usize;
                        let (w, h) = (px.width as usize, px.height as usize);
                        for (half, rows) in [("upper", 0..h / 2), ("lower", h / 2..h)] {
                            let mut sum = [0f64; 4];
                            let mut hist = [[0usize; 8]; 4];
                            let mut cnt = 0usize;
                            for y in rows {
                                for x in 0..w {
                                    let t = &px.data[(y * w + x) * 4..(y * w + x) * 4 + 4];
                                    for c in 0..4 {
                                        sum[c] += f64::from(t[c]);
                                        hist[c][usize::from(t[c] / 32)] += 1;
                                    }
                                    cnt += 1;
                                }
                            }
                            println!("  {half}: mean {:?}", sum.map(|v| v / cnt.max(1) as f64));
                            for c in 0..4 {
                                println!("    ch{c} hist {:?}", hist[c]);
                            }
                        }
                        let _ = n;
                        // A few texels from the middle.
                        let mid = (h / 4) * w + w / 2;
                        println!("  sample {:?}", &px.data[mid * 4..mid * 4 + 32]);
                    }
                }
                Err(e) => println!("page {i} {what}: {e}"),
            }
        }
    }
}
