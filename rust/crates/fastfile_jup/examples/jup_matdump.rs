//! Dumps materials of a zone image: header words, the bytes between header
//! and picture table, and each picture's usage and format:
//! `jup_matdump <file.zone> [min pictures] [count]`.
use fastfile_jup::image;
use fastfile_jup::map::ZoneImage;
use fastfile_jup::map::materials::{material_headers, material_table, picture_table};
use fastfile_jup::ptr::{FOLLOW, INSERT};

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = ZoneImage::parse(std::fs::read(&a[1]).expect("read")).expect("zone");
    let min: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let want: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
    let z = &zone.bytes;
    let mats = zone.types.iter().filter(|&&t| t == 11).count();
    let imgs = zone.types.iter().filter(|&&t| t == 21).count();
    let heads = material_headers(z, mats).expect("headers");
    let run = image::ImageRun::find(z, imgs).expect("run");
    let (_, _, base) = material_table(&zone).expect("table");
    let mut nth = vec![usize::MAX; zone.types.len()];
    let mut k = 0;
    for (i, &t) in zone.types.iter().enumerate() {
        if t == 21 {
            nth[i] = k;
            k += 1;
        }
    }
    let asset_of = |v: u64| -> Option<usize> {
        let o = ((v & 0xffff_ffff) as usize).checked_sub(1)?;
        let rel = o.checked_sub(base)?;
        (rel % 16 == 0 && rel / 16 < zone.types.len()).then_some(rel / 16)
    };
    let mut shown = 0;
    for &h in &heads {
        let pics = usize::from(z[h + 0x1b]);
        if pics < min || !matches!(u64_at(z, h + 0x30), FOLLOW | INSERT) {
            continue;
        }
        let t = picture_table(z, h);
        if a.get(4).is_some_and(|s| s == "gun") {
            // A gun's pictures: a BC7 part mask (usage 0x32), a 0x36 and a
            // normal map (0x37).
            let info: Vec<(u8, u8)> = (0..pics)
                .filter_map(|i| {
                    let x = asset_of(u64_at(z, t + 8 * i)).filter(|&x| zone.types[x] == 21)?;
                    let r = image::parse(z, *run.at.get(nth[x])?).ok().flatten()?;
                    Some((r.usage, r.format))
                })
                .collect();
            let has = |u: u8, f: Option<u8>| info.iter().any(|&(x, y)| x == u && f.is_none_or(|f| f == y));
            if !(has(0x32, Some(0x30)) && has(0x36, None) && has(0x37, None)) {
                continue;
            }
        }
        println!(
            "material {:016x} at {h:#x} pics {pics} consts {} head {:02x?}",
            u64_at(z, h),
            z[h + 0x1c],
            &z[h + 0x10..h + 0x20]
        );
        println!(
            "  ptrs +20 {:#x} +28 {:#x} +38 {:#x} +40 {:#x}",
            u64_at(z, h + 0x20),
            u64_at(z, h + 0x28),
            u64_at(z, h + 0x38),
            u64_at(z, h + 0x40)
        );
        for o in [0x20usize, 0x28, 0x38, 0x40] {
            let v = u64_at(z, h + o);
            if v >> 32 == 0x0f && v & 1 == 1 {
                let at = (v & 0xffff_ffff) as usize - 1 - base + zone.list_at;
                println!("  +{o:02x} -> {at:#x}: {:02x?}", &z[at..at + 96]);
            }
        }
        println!("  between header and table: {:02x?}", &z[h + 0x78..t]);
        for i in 0..pics {
            let v = u64_at(z, t + 8 * i);
            let desc = asset_of(v)
                .filter(|&x| zone.types[x] == 21)
                .and_then(|x| run.at.get(nth[x]))
                .and_then(|&at| image::parse(z, at).ok().flatten())
                .map_or("?".into(), |r| {
                    format!(
                        "{:016x} {}x{} fmt {:#x} usage {:#x}",
                        r.hash, r.width, r.height, r.format, r.usage
                    )
                });
            println!("  pic {i}: {v:#x} {desc}");
        }
        println!(
            "  after table: {:02x?}",
            &z[t + 8 * pics..(t + 8 * pics + 48).min(z.len())]
        );
        shown += 1;
        if shown >= want {
            break;
        }
    }
}
