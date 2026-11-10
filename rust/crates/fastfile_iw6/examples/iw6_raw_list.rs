//! Lists raw files and scripts of zones: `iw6_raw_list <zone.ff>...`
fn main() {
    for path in std::env::args().skip(1) {
        let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
        let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
        println!("{path}: walked {}/{} {:?}", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error));
        for r in &cap.raw_files {
            println!("  raw {} len {} data {}", r.name, r.len, r.data.is_some());
        }
        for r in &cap.script_files {
            println!("  script {} len {} buf {}", r.name, r.len, r.buffer.is_some());
        }
    }
}
