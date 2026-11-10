//! `t7_tables <zone.ff>... [--dump <file>]`: the string tables a zone's
//! walk reaches (name, columns, rows). `--dump` writes them all to a text
//! file: per table a `T\t<name>\t<columns>` line, then a `C\t<cell>` line
//! per cell, row by row (the sim's headless Zombies test reads it).
fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dump = args.iter().position(|a| a == "--dump").map(|i| {
        let f = args.get(i + 1).cloned().expect("--dump <file>");
        args.drain(i..i + 2);
        f
    });
    let mut out = String::new();
    for path in &args {
        let z = fastfile_t7::Zone::open(std::path::Path::new(path)).expect("zone");
        let w = fastfile_t7::walk::walk_with(&z, fastfile_t7::walk::Options { resync: true, keep_anims: true });
        println!("{path}: {} assets walked, stop {:?}, {} tables", w.walked.len(), w.stop, w.capture.tables.len());
        for (name, cols, cells) in &w.capture.tables {
            println!("  {name}: {cols} columns, {} rows", cells.len() / (*cols).max(1));
            out.push_str(&format!("T\t{name}\t{cols}\n"));
            for c in cells {
                out.push_str(&format!("C\t{}\n", c.replace('\n', " ")));
            }
        }
    }
    if let Some(f) = dump {
        std::fs::write(f, out).expect("write");
    }
}
