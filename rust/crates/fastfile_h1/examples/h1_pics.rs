//! A map's loading picture as a PNG, the way the map picker reads it:
//! `cargo run -p fastfile_h1 --example h1_pics -- <map zone.ff> <out.png>`.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(zone), Some(out)) = (a.first(), a.get(1)) else {
        eprintln!("usage: h1_pics <map zone.ff> <out.png>");
        std::process::exit(2);
    };
    let zone = std::path::Path::new(zone);
    let stem = zone.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let names = [format!("loadscreen_{stem}"), format!("preview_{stem}")];
    let (w, h, rgba) = fastfile_h1::ui_image::read_map_picture(zone, zone.parent(), &names).expect("no loading picture");
    let f = std::io::BufWriter::new(std::fs::File::create(out).expect("out"));
    let mut e = png::Encoder::new(f, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png");
    println!("{w}x{h} -> {out}");
}
