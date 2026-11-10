//! Prints a zone's string tables whose names contain a pattern (all
//! names when none is given).
//!
//! `cargo run -p render_h1 --example h1_tables -- <zone.ff> [pattern]`

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let z = fastfile_h1::Zone::open(std::path::Path::new(&a[0])).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for (name, dims, px) in &cap.cluts {
        println!("clut {name} {dims:?} {}", px.map_or(0, |p| p.len));
    }
    for (name, columns, cells) in &cap.string_tables {
        match a.get(1) {
            None => println!("{name} ({columns} columns, {} cells)", cells.len()),
            Some(p) if name.contains(p.as_str()) => {
                println!("==== {name}");
                for row in cells.chunks((*columns).max(1)) {
                    println!("{}", row.join(","));
                }
            }
            _ => {}
        }
    }
}
