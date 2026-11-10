//! Prints the string tables of a Ghosts zone whose name contains a word:
//! `iw6_stringtable_probe <zone.ff> <word> [max rows]` (rows as `|`-separated cells).
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let word = a.next().unwrap_or_default().to_ascii_lowercase();
    let max: usize = a.next().and_then(|s| s.parse().ok()).unwrap_or(40);
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    println!("walked {}/{} {:?}; {} string tables", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error), cap.string_tables.len());
    for t in cap.string_tables.iter().filter(|t| t.name.to_ascii_lowercase().contains(&word)) {
        println!("== {} ({} rows x {} columns)", t.name, t.rows, t.columns);
        for r in 0..t.rows.min(max) {
            let row: Vec<&str> = (0..t.columns).map(|c| t.cell(r, c)).collect();
            println!("{r}: {}", row.join("|"));
        }
    }
}
