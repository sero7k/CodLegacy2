//! `t7_depends <zone.ff>...`: the zones each zone names as its dependencies.
fn main() {
    for p in std::env::args().skip(1) {
        let Ok(z) = fastfile_t7::Zone::open(std::path::Path::new(&p)) else { continue };
        let d: Vec<String> = z.assets.depends.iter().flatten().cloned().collect();
        println!("{p}: {d:?}");
    }
}
