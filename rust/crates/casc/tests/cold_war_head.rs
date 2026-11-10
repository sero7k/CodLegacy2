//! Against the owner's cut of the first 1 MiB of `data.000` (`$CW_DATA000_HEAD`, default the
//! staging path); skipped when absent. Offsets are from the install's `.idx` files.
use casc::{blte, tvfs};

fn head() -> Option<Vec<u8>> {
    let p = std::env::var("CW_DATA000_HEAD").unwrap_or("/mnt/user-data/uploads/Documents/cw_data000_head.bin".into());
    std::fs::read(p).ok()
}

#[test]
fn manifests_decode_and_tvfs_lists_the_zones() {
    let Some(h) = head() else { eprintln!("skipped: no data.000 head"); return };
    let mut m = blte::Modes::default();
    let enc = blte::decode_entry(&h[480..480 + 413_153], &mut m).unwrap();
    assert_eq!(&enc[..2], b"EN");
    assert_eq!(enc.len(), 412_959);
    let root = blte::decode_entry(&h[418_319..418_319 + 266_738], &mut m).unwrap();
    assert_eq!(root.len(), 403_675);
    assert_eq!(m.encrypted, 0);
    let files = tvfs::parse(&root).unwrap();
    assert_eq!(files.len(), 6415);
    let ff = files.iter().find(|f| f.path == "zone/mp_sm_central.ff").unwrap();
    assert_eq!(ff.size(), 5_962_560);
    assert_eq!(casc_hex(&ff.spans[0].ekey), "92c13b8c07abf72f27");
    assert_eq!(files.iter().filter(|f| f.path.starts_with("zone/mp_") && f.path.ends_with(".ff")).count(), 37);
}

fn casc_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
