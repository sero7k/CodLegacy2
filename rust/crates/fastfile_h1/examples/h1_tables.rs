//! A zone's string tables: name, size and the first row.
//!
//! `cargo run -p fastfile_h1 --release --example h1_tables -- <zone.ff> [filter]`
fn main() {
    let mut a = std::env::args().skip(1);
    let p = a.next().expect("zone");
    let filter = a.next().unwrap_or_default();
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for (name, columns, cells) in &cap.string_tables {
        if !name.contains(&filter) {
            continue;
        }
        let rows = if *columns == 0 { 0 } else { cells.len() / *columns as usize };
        let first: Vec<&str> = cells.iter().take(*columns as usize).map(String::as_str).collect();
        println!("{name}: {columns} columns x {rows} rows; first row {first:?}");
    }
}
