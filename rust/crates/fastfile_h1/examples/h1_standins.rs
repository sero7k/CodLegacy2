//! The pictures of a gun's view model whose zone copy is a 1 x 1 stand-in
//! (`fastfile_h1::shared::is_standin`), per material and texture slot, and
//! whether the retail zone in `<retail zone dir>` holds the real one.
//!
//! `cargo run -p fastfile_h1 --example h1_standins -- <retail zone dir> <gun substr> <zone.ff>...`

use std::collections::BTreeSet;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(retail), Some(gun)) = (a.first(), a.get(1)) else {
        eprintln!("usage: h1_standins <retail zone dir> <gun substr> <zone.ff>...");
        std::process::exit(2);
    };
    let retail = Path::new(retail);
    for path in &a[2..] {
        let zone = fastfile_h1::Zone::open(Path::new(path)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        let all: usize = cap.images.iter().filter(|r| fastfile_h1::shared::is_standin(r)).count();
        println!("{path}: {all} stand-in pictures in the zone");
        println!("{:?}", fastfile_h1::shared::load_standins(retail, &cap.images));
        let mut paks = fastfile_h1::image::ImagePaks::for_zone(Path::new(path), Some(retail));
        for w in cap.weapons.iter().filter(|w| w.name.contains(gun.as_str())) {
            let mut seen = BTreeSet::new();
            for id in w.gun_models.iter().flatten() {
                let Some(m) = cap.xmodels.get(id.index as usize) else { continue };
                println!("{} view model {}", w.name, m.name);
                let mut mats: Vec<_> = m.materials.iter().flatten().copied().collect();
                for p in m.composite.iter().flatten() {
                    if let Some(pm) = cap.xmodels.get(p.index as usize) {
                        mats.extend(pm.materials.iter().flatten().copied());
                    }
                }
                for mat in mats {
                    if !seen.insert(mat.index) {
                        continue;
                    }
                    let Some(mr) = cap.materials.get(mat.index as usize) else { continue };
                    let tech = mr.techset.map(|t| cap.name_of(t)).unwrap_or("-");
                    println!("  material {} tech {}", cap.name_of(mat), tech);
                    for t in &mr.textures {
                        let Some(i) = t.image else { continue };
                        let rec = &cap.images[i.index as usize];
                        let standin = fastfile_h1::shared::is_standin(rec);
                        let real = standin
                            && fastfile_h1::shared::standin_pixels(&rec.name, &mut paks, 256)
                                .is_some_and(|r| r.is_ok());
                        // H1_DUMP=<dir>: the picture as a PNG (alpha dropped to
                        // 255 in a second file `<name>_a.png`, the alpha channel).
                        if let Some(dir) = std::env::var_os("H1_DUMP")
                            && !standin
                            && let Ok(px) = fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, 256)
                            && let Ok((w, h, rgba)) = fastfile_h1::image::rgba8_level0(&px)
                        {
                            let name = cap.name_of(i).trim_start_matches(',').replace(['/', '~', '&'], "_");
                            for (suffix, alpha_only) in [("", false), ("_a", true)] {
                                let data: Vec<u8> = rgba
                                    .chunks_exact(4)
                                    .flat_map(|c| if alpha_only { [c[3], c[3], c[3], 255] } else { [c[0], c[1], c[2], 255] })
                                    .collect();
                                let f = std::fs::File::create(Path::new(&dir).join(format!("{name}{suffix}.png"))).expect("png");
                                let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                                e.set_color(png::ColorType::Rgba);
                                e.set_depth(png::BitDepth::Eight);
                                e.write_header().and_then(|mut x| x.write_image_data(&data)).expect("png");
                            }
                        }
                        println!(
                            "    hash {:08x} sem {} {} {}x{} streamed {} fmt {}{}",
                            t.name_hash,
                            t.semantic,
                            cap.name_of(i),
                            rec.width,
                            rec.height,
                            rec.streamed,
                            rec.format,
                            if rec.width == 1 && rec.height == 1 {
                                let px = fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, 256)
                                    .map(|p| p.data[..4.min(p.data.len())].to_vec());
                                let tag = match (standin, real) {
                                    (true, true) => "  STAND-IN (retail has it)",
                                    (true, false) => "  STAND-IN (NOT in retail zones read)",
                                    _ => "",
                                };
                                format!("  pixel {px:?}{tag}")
                            } else {
                                String::new()
                            }
                        );
                    }
                }
            }
        }
    }
}
