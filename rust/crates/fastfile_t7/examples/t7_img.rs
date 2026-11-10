//! A map zone's picture as a PNG: `t7_img <map.ff> <out dir> <name>...`
//! (zone pixels, the common zones', or the map's `.xpak`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let out = std::path::PathBuf::from(&a[2]);
    for name in &a[3..] {
        match map.image(name, 4096) {
            Ok(l) => {
                let Some((w, h, px)) = fastfile_t7::texture::level_rgba(&l, 0) else { println!("{name}: no rgba"); continue };
                let path = out.join(format!("{}.png", name.trim_start_matches(',').replace('/', "_")));
                let f = std::fs::File::create(&path).expect("png");
                let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png data");
                println!("{name}: {w}x{h} -> {}", path.display());
            }
            Err(e) => println!("{name}: {e}"),
        }
    }
}
