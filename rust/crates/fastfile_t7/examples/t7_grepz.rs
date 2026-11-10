//! `t7_grepz <text> <zone.ff>...`: how often the text occurs in each inflated zone.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for z in &a[2..] {
        let Ok(zone) = fastfile_t7::Zone::open(std::path::Path::new(z)) else { continue };
        let n = zone.image.windows(a[1].len()).filter(|w| *w == a[1].as_bytes()).count();
        println!("{z}: {n}");
    }
}
