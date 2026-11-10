//! Writes each script of a zone as `<out>/<name>.stream` and `.code`: `gsc_extract <zone.ff> <out dir>`.
use std::io::Read;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&a[0])).expect("open");
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    println!("walked {}/{} stop {:?}", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error));
    let n_sf = zone.assets.assets.iter().filter(|x| x.asset_type() == Some(fastfile_iw6::AssetType::ScriptFile)).count();
    println!("script file assets in the zone header: {n_sf}");
    if std::env::var("LIST").is_ok() { for (i, x) in zone.assets.assets.iter().enumerate() { if x.asset_type() == Some(fastfile_iw6::AssetType::ScriptFile) { println!("  asset {i} inline {}", x.is_inline()); } } }
    std::fs::create_dir_all(&a[1]).unwrap();
    for sf in &cap.script_files {
        let (Some(b), Some(c)) = (sf.buffer, sf.bytecode) else { println!("{} no data", sf.name); continue };
        let mut out = Vec::new();
        let _ = flate2::read::ZlibDecoder::new(b.get(&zone.image)).read_to_end(&mut out);
        let n = sf.name.replace('/', "__");
        std::fs::write(format!("{}/{n}.stream", a[1]), &out).unwrap();
        std::fs::write(format!("{}/{n}.code", a[1]), c.get(&zone.image)).unwrap();
        println!("{} comp {} len {} inflated {} code {}", sf.name, sf.compressed_len, sf.len, out.len(), sf.bytecode_len);
    }
}
