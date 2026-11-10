//! Histogram of the pictures' usage byte and format in a zone image:
//! `jup_usage <file.zone>`.
use fastfile_jup::image;
use fastfile_jup::map::ZoneImage;

fn main() {
    let path = std::env::args().nth(1).expect("usage: jup_usage <file.zone>");
    let zone = ZoneImage::parse(std::fs::read(&path).expect("read")).expect("zone");
    let imgs = zone.types.iter().filter(|&&t| t == 21).count();
    let Some(run) = image::ImageRun::find(&zone.bytes, imgs) else {
        println!("no image run");
        return;
    };
    let mut h: std::collections::BTreeMap<(u8, u8), usize> = Default::default();
    for &at in &run.at {
        if let Ok(Some(r)) = image::parse(&zone.bytes, at) {
            *h.entry((r.usage, r.format)).or_default() += 1;
        }
    }
    for ((u, f), n) in h {
        println!("usage {u:#04x} format {f:#04x}: {n}");
    }
}
