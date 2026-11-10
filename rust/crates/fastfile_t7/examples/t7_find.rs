//! `t7_find <name substring> <zone.ff>...`: the technique sets and images
//! whose names contain the substring, per zone (techniques captured).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for z in &a[2..] {
        let Ok(zone) = fastfile_t7::Zone::open(std::path::Path::new(z)) else { println!("{z}: unreadable"); continue };
        let w = fastfile_t7::walk::walk(&zone);
        for t in w.capture.techsets.iter().filter(|t| t.name.contains(&a[1])) {
            let tech: Vec<usize> = (0..12).filter(|&i| t.techniques[i].is_some()).collect();
            let progs: Vec<usize> = (0..12).filter(|&i| t.programs[i].is_some()).collect();
            println!("{z}: techset {} techniques {tech:?} programs {progs:?}", t.name);
        }
        for i in w.capture.images.iter().filter(|i| i.name.contains(&a[1])) {
            println!("{z}: image {} {}x{} fmt {} pixels {:?} xpak parts {}", i.name, i.width, i.height, i.format, i.pixels, i.xpak.len());
        }
    }
}
