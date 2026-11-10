//! Lists a zone's asset table (index, type, inline or reference):
//! `cargo run -p fastfile_h1 --example h1_assets -- <zone.ff> [from] [to]`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: h1_assets <zone.ff> [from] [to]");
        std::process::exit(2);
    };
    let zone = fastfile_h1::Zone::open(std::path::Path::new(path)).expect("zone");
    let n = zone.assets.assets.len();
    let from = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let to = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(n).min(n);
    for (i, a) in zone.assets.assets.iter().enumerate().take(to).skip(from) {
        let t = a.asset_type().map(|t| t.name()).unwrap_or("?");
        println!("{i} {t} {:?}", a.header);
    }
}
