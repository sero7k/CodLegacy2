//! World surfaces and static models by their material's camera region.
//!
//! `cargo run -p render_h1 --release --example h1_regions -- <map.ff>`
use std::collections::BTreeMap;
fn main() {
    let p = std::env::args().nth(1).expect("zone");
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    let map = fastfile_h1::map::read_map(&cap, &z.image, &z.assets.script_strings);
    let w = map.world.expect("world");
    let mut by: BTreeMap<u8, BTreeMap<String, usize>> = BTreeMap::new();
    for s in &w.surfaces {
        let Some(m) = s.material else { continue };
        let mat = &cap.materials[m.index as usize];
        *by.entry(mat.camera_region).or_default().entry(cap.name_of(m).to_string()).or_default() += 1;
    }
    for (r, names) in by {
        let n: usize = names.values().sum();
        println!("region {r}: {n} world surfaces, {} materials", names.len());
        let mut v: Vec<_> = names.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (k, c) in v.iter().take(if r == 12 { 25 } else { 4 }) {
            println!("   {c} x {k}");
        }
    }
}
