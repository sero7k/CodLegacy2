//! Walks H1 zones and says whether each walk is exact (stream end = zone
//! end, VIRTUAL end = the XFile's block size): `h1_walk_check <zone.ff>...
//! [H1_FROM=<asset index>]` also lists 40 asset starts from that index;
//! `H1_HEX=<from>,<to>` hex-dumps a file range.
fn main() {
    let from: Option<usize> = std::env::var("H1_FROM").ok().and_then(|v| v.parse().ok());
    for path in std::env::args().skip(1) {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(&path)).expect("open");
        let mut w =
            fastfile_h1::walk::Walker::new(&zone.image, &zone.assets, &zone.header.stream_files);
        let stop = w.walk().err();
        println!(
            "{path}: walked {}/{} stop {:?}; pos {:#x} of {:#x}",
            w.cap.walked,
            zone.assets.assets.len(),
            stop,
            w.stream_pos(),
            zone.image.len()
        );
        let cap = &w.cap;
        if let Some(f) = from {
            for i in f..(f + 40).min(cap.starts.len()) {
                let ty = zone.assets.assets[i].asset_type().map(|t| t.name());
                println!("  {i:5} {ty:?} at {:#x}", cap.starts[i]);
            }
        }
        if let Ok(r) = std::env::var("H1_HEX")
            && let Some((a, b)) = r.split_once(',')
        {
            let a = usize::from_str_radix(a.trim_start_matches("0x"), 16).unwrap();
            let b = usize::from_str_radix(b.trim_start_matches("0x"), 16).unwrap();
            for row in (a..b).step_by(16) {
                let bytes = &zone.image[row..(row + 16).min(zone.image.len())];
                println!(
                    "HEX {row:#x}: {}",
                    bytes
                        .iter()
                        .map(|x| format!("{x:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
    }
}
