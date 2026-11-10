//! Image and material names in zones matching a substring: `t7_names
//! <substring> <zone.ff>...`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let want = a[1].to_ascii_lowercase();
    for z in &a[2..] {
        let Ok(zone) = fastfile_t7::Zone::open(std::path::Path::new(z)) else {
            println!("{z}: not opened");
            continue;
        };
        let w = fastfile_t7::walk::walk(&zone);
        for i in &w.capture.images {
            if i.name.to_ascii_lowercase().contains(&want) {
                println!("{z}: image {} {}x{}", i.name, i.width, i.height);
            }
        }
        for m in &w.capture.materials {
            if m.name.to_ascii_lowercase().contains(&want) {
                println!("{z}: material {} {:?}", m.name, m.textures);
            }
        }
    }
}
