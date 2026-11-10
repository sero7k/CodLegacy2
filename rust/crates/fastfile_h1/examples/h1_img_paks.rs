//! Which `imagefile<N>.pak` each of a zone's streamed images lives in:
//! `h1_img_paks <zone.ff> [name-filter]`; then a count per pak.
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&a[0])).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
    let f = a.get(1).cloned().unwrap_or_default();
    let mut by: BTreeMap<u16, (usize, u64)> = BTreeMap::new();
    for i in &cap.images {
        for s in i.stream_files.iter().flatten() {
            if !f.is_empty() && i.name.contains(&f) {
                println!("{} -> imagefile{} {}..{}", i.name, s.file_index, s.offset, s.offset_end);
            }
            let e = by.entry(s.file_index).or_default();
            e.0 += 1;
            e.1 += s.offset_end - s.offset;
        }
    }
    for (k, (n, b)) in by {
        println!("pak {k}: {n} parts, {} MB", b / 1_000_000);
    }
}
