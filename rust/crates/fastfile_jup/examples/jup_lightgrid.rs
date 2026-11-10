//! Shows a map zone's light grid header block and what surrounds it:
//! `jup_lightgrid <map.zone> [before] [after]` (bytes around the block).
use fastfile_jup::image;
use fastfile_jup::map::ZoneImage;
use fastfile_jup::map::lighting;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = ZoneImage::parse(std::fs::read(&a[1]).expect("read")).expect("zone");
    let before: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(256);
    let after: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let imgs = zone.types.iter().filter(|&&t| t == 21).count();
    let run = image::ImageRun::find(&zone.bytes, imgs).expect("image run");
    let mut report = Vec::new();
    let l = lighting::read(&zone, &run, &mut report);
    for r in &report {
        println!("{r}");
    }
    let Some(at) = l.grids.block_at else {
        println!("no header block");
        return;
    };
    println!("block at {at:#x}, list at {:#x}", zone.list_at);
    let z = &zone.bytes;
    let from = at.saturating_sub(before);
    for (i, row) in z[from..(at + after).min(z.len())].chunks(32).enumerate() {
        let o = from + 32 * i;
        let f: Vec<String> = row
            .chunks(4)
            .filter(|c| c.len() == 4)
            .map(|c| {
                let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                if v.is_finite() && v != 0.0 && (1e-3..1e7).contains(&v.abs()) { format!("{v:.3}") } else { String::new() }
            })
            .collect();
        println!("{o:#09x} {:<80} {}", row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "), f.join(" "));
    }
}
