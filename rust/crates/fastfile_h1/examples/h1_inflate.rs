//! Writes a zone's inflated image (the loader's stream) to a file, for
//! inspection: `cargo run -p fastfile_h1 --example h1_inflate -- <zone.ff> <out>`.

fn main() {
    let mut args = std::env::args_os().skip(1);
    let (Some(src), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: h1_inflate <zone.ff> <out>");
        std::process::exit(2);
    };
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&src)).expect("zone");
    std::fs::write(&out, &zone.image).expect("write");
    println!(
        "{} bytes, body at {:#x}, {} assets, {} stream files",
        zone.image.len(),
        zone.assets.body_offset,
        zone.assets.assets.len(),
        zone.header.stream_files.len()
    );
}
