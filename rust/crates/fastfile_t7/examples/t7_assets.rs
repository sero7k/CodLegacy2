//! `t7_assets <zone.ff> <from> <to>`: the zone's asset table entries in a range (type, inline).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let (from, to): (usize, usize) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    for (i, e) in z.assets.assets.iter().enumerate().skip(from).take(to - from) {
        println!("{i} {e:?}");
    }
}
