//! Lists a zone's raw files (and prints those whose names contain a pattern).
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let z = fastfile_h1::Zone::open(std::path::Path::new(&a[0])).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for r in &cap.raw_files {
        println!("{}", r.name);
    }
    for pat in a.iter().skip(1) {
        for r in cap
            .raw_files
            .iter()
            .filter(|r| r.name.contains(pat.as_str()))
        {
            if let Some(b) = fastfile_h1::map::raw_file(&cap, &z.image, &r.name) {
                println!("==== {}\n{}", r.name, String::from_utf8_lossy(&b));
            }
        }
    }
}
