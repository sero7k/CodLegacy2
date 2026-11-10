//! Decodes a zone to a file and lists its asset types:
//! `jup_zonedump <zone .ff> <out .zone>` (needs the Oodle backend, see
//! `docs/JUP.md`). The decoded zone is the input of the other offline probes
//! (`jup_zonetypes`, `jup_vfx`, ...); delete it when done.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (Some(src), Some(out)) = (a.get(1), a.get(2)) else {
        eprintln!("usage: jup_zonedump <zone .ff> <out .zone>");
        std::process::exit(2);
    };
    let path = std::path::Path::new(src);
    let install = path
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or(std::path::Path::new("."));
    let oodle = fastfile_jup::oodle::load(install).expect("oodle");
    let image = fastfile_jup::map::read_image(path, oodle.as_ref()).expect("zone");
    std::fs::write(out, &image.bytes).expect("write");
    let mut kinds: std::collections::BTreeMap<u64, usize> = Default::default();
    for t in &image.types {
        *kinds.entry(u64::from(*t)).or_default() += 1;
    }
    println!("{} bytes, {} assets", image.bytes.len(), image.types.len());
    for (k, n) in kinds {
        println!("type {k:4} x {n}");
    }
}
