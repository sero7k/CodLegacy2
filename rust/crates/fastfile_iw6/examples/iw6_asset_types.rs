//! Counts asset types per zone: `iw6_asset_types <zone.ff>...`
use std::collections::BTreeMap;
fn main() {
    for path in std::env::args().skip(1) {
        let zone = match fastfile_iw6::Zone::open(std::path::Path::new(&path)) {
            Ok(z) => z,
            Err(e) => {
                println!("{path}: {e:?}");
                continue;
            }
        };
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for a in &zone.assets.assets {
            *m.entry(format!("{:?}", a.asset_type())).or_default() += 1;
        }
        println!("{path}: {} assets: {:?}", zone.assets.assets.len(), m);
    }
}
