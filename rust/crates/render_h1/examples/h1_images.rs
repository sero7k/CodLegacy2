//! Image records whose names contain a pattern: format, size, levels, streamed.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let z = fastfile_h1::Zone::open(std::path::Path::new(&a[0])).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for r in cap
        .images
        .iter()
        .filter(|r| a[1..].iter().any(|p| r.name.contains(p.as_str())))
    {
        println!(
            "{} fmt {} {}x{} levels {} map_type {} semantic {} streamed {} inline {}",
            r.name,
            r.format,
            r.width,
            r.height,
            r.level_count,
            r.map_type,
            r.semantic,
            r.streamed,
            r.pixels.is_some()
        );
    }
}
