//! Asset type histogram of a decoded zone: `jup_zonetypes <file.zone> [--runs]`
//! (`--runs`: the asset list as runs of one type, in order).
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = std::fs::read(&a[1]).expect("read");
    let h = fastfile_jup::zone::parse_header(&z).expect("header");
    if a.iter().any(|x| x == "--runs") {
        let mut i = 0;
        while i < h.assets.len() {
            let k = h.assets[i].kind;
            let mut j = i;
            while j < h.assets.len() && h.assets[j].kind == k {
                j += 1;
            }
            println!("{i:6} type {k:3} x {}", j - i);
            i = j;
        }
        return;
    }
    let mut m: BTreeMap<u64, usize> = BTreeMap::new();
    for e in &h.assets {
        *m.entry(e.kind).or_default() += 1;
    }
    println!("{} assets, data starts at {:#x}", h.assets.len(), h.data_start);
    for (k, n) in m {
        println!("type {k:4} x {n}");
    }
}
