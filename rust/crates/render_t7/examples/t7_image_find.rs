//! `t7_image_find <zone.ff>... -- <substring>`: image records whose name
//! contains the substring (format, size, levels, where the pixels are).
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let split = a.iter().position(|x| x == "--").unwrap_or(a.len());
    let pat = a.get(split + 1).cloned().unwrap_or_default();
    for zp in &a[..split] {
        let Ok(z) = fastfile_t7::Zone::open(std::path::Path::new(zp)) else { continue };
        let w = fastfile_t7::walk::walk(&z);
        for im in w.capture.images.iter().filter(|i| i.name.contains(&pat)) {
            println!("{zp}: {} fmt {} {}x{}x{} levels {} pixels {:?} xpak {}", im.name, im.format, im.width, im.height, im.depth, im.levels, im.pixels.map(|p| p.1), im.xpak.len());
        }
    }
}
