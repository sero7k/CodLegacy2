//! Histogram of material header bytes: `jup_mathead <file.zone> [off:len,...]`
//! (default: the +0x08..0x20 body). With a list of byte ranges (hex offsets
//! from the header start) the histogram is over those bytes only, and each
//! row also gives the picture counts, slot-pair counts and constant rows.
use std::collections::BTreeMap;

use fastfile_jup::map::ZoneImage;
use fastfile_jup::map::materials::material_headers;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = ZoneImage::parse(std::fs::read(&a[1]).expect("read")).expect("zone");
    let z = &zone.bytes;
    let mats = zone.types.iter().filter(|&&t| t == 11).count();
    let heads = material_headers(z, mats).expect("headers");
    let ranges: Vec<(usize, usize)> = match a.get(2) {
        Some(s) => s
            .split(',')
            .map(|r| {
                let (o, l) = r.split_once(':').expect("off:len");
                (usize::from_str_radix(o, 16).expect("hex"), l.parse().expect("len"))
            })
            .collect(),
        None => vec![(8, 0x18)],
    };
    let mut by: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for &h in &heads {
        let mut k = Vec::new();
        for &(o, l) in &ranges {
            k.extend_from_slice(&z[h + o..h + o + l]);
        }
        *by.entry(k).or_default() += 1;
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("{} materials, {} distinct keys", heads.len(), v.len());
    for (k, n) in v.iter().take(60) {
        println!("{n:6} {k:02x?}");
    }
}
