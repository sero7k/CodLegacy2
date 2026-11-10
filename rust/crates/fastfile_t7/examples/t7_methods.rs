//! `t7_methods <map.ff> [out dir]`: each technique set's G-buffer technique draw methods (programs), optionally dumped.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&zone);
    for t in &w.capture.techsets {
        let Some(info) = t.techniques[3].as_ref() else { continue };
        let ms: Vec<String> = info.methods.iter().enumerate().filter_map(|(i, m)| m.as_ref().map(|m| format!("{i}:{:?}", m.programs))).collect();
        println!("{} {}", t.name, ms.join(" "));
        if let Some(dir) = a.get(2) {
            for (i, m) in info.methods.iter().enumerate() {
                let Some(m) = m else { continue };
                for (tag, p) in ["vs", "ps"].iter().zip(m.programs) {
                    if let Some((at, n)) = p {
                        let f = format!("{dir}/{}_m{i}_{tag}.dxbc", t.name.replace(['/', '#'], "_"));
                        std::fs::write(f, &zone.image[at..at + n]).ok();
                    }
                }
            }
        }
    }
}
