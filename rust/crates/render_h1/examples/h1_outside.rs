//! World surfaces outside every drawn range and not sky: count by material.
//!
//! `cargo run -p render_h1 --release --example h1_outside -- <map.ff>`
use std::collections::BTreeMap;
fn main() {
    let p = std::env::args().nth(1).expect("zone");
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    let map = fastfile_h1::map::read_map(&cap, &z.image, &z.assets.script_strings);
    let w = map.world.expect("world");
    let inr = |i: usize| w.ranges.iter().any(|&(b, e)| (b as usize..e as usize).contains(&i));
    let sky: std::collections::HashSet<u32> = w.sky_surfaces.iter().copied().collect();
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    let (mut n, mut skyn) = (0, 0);
    for i in 0..w.surfaces.len() {
        if inr(i) { continue; }
        if sky.contains(&(i as u32)) { skyn += 1; continue; }
        n += 1;
        let nm = w.surfaces[i].material.map(|m| cap.name_of(m).to_string()).unwrap_or_default();
        *by.entry(nm).or_default() += 1;
    }
    println!("outside and not sky: {n}, sky {skyn}");
    for (k, v) in by { println!("  {v} x {k}"); }
}
