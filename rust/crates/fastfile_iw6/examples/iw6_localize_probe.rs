//! Lists the localized strings of a Ghosts zone: `iw6_localize_probe <zone.ff> [word]`.
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let word = a.next().unwrap_or_default().to_ascii_lowercase();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    println!("walked {}/{} {:?}; {} strings", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error), cap.localized.len());
    for (k, v) in cap.localized.iter().filter(|(k, v)| k.to_ascii_lowercase().contains(&word) || v.to_ascii_lowercase().contains(&word)) {
        println!("{k} = {v:?}");
    }
}
