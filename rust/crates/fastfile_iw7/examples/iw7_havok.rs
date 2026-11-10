//! A map's Havok world shape (its collision): `iw7_havok <zone.ff> [out.bin]`
//! prints each clip map's shape size and writes the first to `out.bin`.
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("usage: iw7_havok <zone.ff> [out.bin]");
    let out = a.next();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&path)).expect("open");
    let (w, _) = fastfile_iw7::walk::walk_fitted(&zone);
    for c in &w.cap.clip_maps {
        match c.havok_world_shape {
            Some(s) => {
                println!(
                    "clipmap {:?}: {} static models, havok shape {} bytes at {:#x}",
                    c.name, c.static_model_count, s.len, s.pos
                );
                if let Some(out) = &out {
                    std::fs::write(out, &zone.image[s.pos..s.pos + s.len]).expect("write");
                }
            }
            None => println!(
                "clipmap {:?}: {} static models, no havok shape",
                c.name, c.static_model_count
            ),
        }
    }
}
