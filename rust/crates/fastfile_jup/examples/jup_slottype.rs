//! For every material of a zone image with a following picture table: its
//! slot -> picture pairs, and per slot id the usage/format of the pictures
//! it names (pictures shared by 200+ materials are the defaults).
//! `jup_slottype <file.zone> [class bytes to key on, e.g. 12..16]`: the same, per
//! material class (the header bytes at `+0x0c..+0x18` by default).
use std::collections::{BTreeMap, HashMap};

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
    let mut users: HashMap<u64, usize> = HashMap::new();
    let mut rows: Vec<(usize, Vec<(u8, u64)>)> = Vec::new();
    let (ka, kb) = a.get(2).and_then(|r| r.split_once('-')).map_or((0x0c, 0x18), |(x, y)| (x.parse::<usize>().unwrap(), y.parse::<usize>().unwrap()));
    for &h in &heads {
        let pics = usize::from(z[h + 0x1b]);
        let npairs = usize::from(z[h + 0x18]);
        if pics == 0 || !matches!(u64_at(z, h + 0x30), FOLLOW | INSERT) || !matches!(u64_at(z, h + 0x28), FOLLOW | INSERT) {
            continue;
        }
        let t = picture_table(z, h);
        let ps = t - 2 * npairs;
        let mut v = Vec::new();
        for i in 0..npairs {
            let (s, b) = (z[ps + 2 * i], usize::from(z[ps + 2 * i + 1]));
            if b < pics {
                let p = u64_at(z, t + 8 * b);
                v.push((s, p));
                *users.entry(p).or_default() += 1;
            }
        }
        rows.push((h, v));
    }
    let mut stat: BTreeMap<Vec<u8>, BTreeMap<u8, BTreeMap<(u8, u8), usize>>> = BTreeMap::new();
    let mut size: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for (h, v) in &rows {
        let key = z[h + ka..h + kb].to_vec();
        *size.entry(key.clone()).or_default() += 1;
        for &(s, p) in v {
            if users[&p] > 200 {
                continue;
            }
            let Some(at) = asset_of(p)
                .filter(|&x| zone.types[x] == 21)
                .and_then(|x| run.at.get(nth[x]))
            else {
                continue;
            };
            if let Ok(Some(r)) = image::parse(z, *at) {
                *stat.entry(key.clone()).or_default().entry(s).or_default().entry((r.usage, r.format)).or_default() += 1;
            }
        }
    }
    let mut classes: Vec<_> = size.iter().collect();
    classes.sort_by_key(|x| std::cmp::Reverse(*x.1));
    println!("{} materials in {} classes", rows.len(), classes.len());
    for (key, n) in classes.iter().take(8) {
        println!("class {key:02x?}: {n} materials");
        for (s, m) in &stat[*key] {
            let mut v: Vec<_> = m.iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(*x.1));
            let t: Vec<String> = v.iter().take(4).map(|((u, f), n)| format!("u{u:x}/f{f:x}:{n}")).collect();
            println!("   slot {s:#04x}: {}", t.join(" "));
        }
    }
}
