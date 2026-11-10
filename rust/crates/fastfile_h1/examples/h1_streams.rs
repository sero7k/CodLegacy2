//! Prints a zone's `XStreamFile` table (streamed image pixels): index,
//! file index, flags, range. `cargo run -p fastfile_h1 --example h1_streams -- <zone.ff>`.

fn main() {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: h1_streams <zone.ff>");
        std::process::exit(2);
    };
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&path)).expect("zone");
    let mut files = std::collections::BTreeMap::<u16, (usize, u64)>::new();
    for (i, s) in zone.header.stream_files.iter().enumerate() {
        if i < 16 {
            println!(
                "{i} word0 {:#x} file {} flags {:#x} {:#x}..{:#x}",
                s.word0, s.file_index, s.flags, s.offset, s.offset_end
            );
        }
        let e = files.entry(s.file_index).or_insert((0, 0));
        e.0 += 1;
        e.1 = e.1.max(s.offset_end);
    }
    for (f, (n, end)) in files {
        println!("file index {f}: {n} entries, max end {end:#x}");
    }
}
