//! Against the owner's four cuts (CASC entries) in `$CW_CUTS` (default the staging folder);
//! skipped when absent. No Oodle decoding: layout only.
use fastfile_t9::{fd, kapi, taff};

fn cut(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var("CW_CUTS").unwrap_or("/mnt/user-data/uploads/Documents/cw".into());
    let e = std::fs::read(format!("{dir}/{name}.casc")).ok()?;
    let mut m = casc::blte::Modes::default();
    let b = casc::blte::decode_entry(&e, &mut m).unwrap();
    assert_eq!(m.encrypted, 0);
    Some(b)
}

#[test]
fn sm_central_ff_chain_and_fd_header() {
    let Some(ff) = cut("mp_sm_central.ff") else { eprintln!("skipped: no cuts"); return };
    let h = taff::header(&ff).unwrap();
    assert_eq!((h.zone.as_str(), h.builder.as_str(), h.decoded_size), ("mp_sm_central", "TA-BM-SV-T9-12", 30_178_741));
    let bl = taff::blocks(&ff).unwrap();
    assert_eq!(bl.len(), 29);
    // Every block is an Oodle Kraken stream.
    assert!(bl.iter().all(|b| ff[b.data] == 0x8c && ff[b.data + 1] == 0x06));
    let en = cut("en_mp_sm_central.ff").unwrap();
    assert_eq!(taff::blocks(&en).unwrap().len(), 1);
    let fdb = cut("mp_sm_central.fd").unwrap();
    let f = fd::header(&fdb).unwrap();
    assert_eq!((f.patched.builder.as_str(), f.base.builder.as_str()), ("TA-BM-SV-T9-23", "TA-BM-SV-T9-12"));
    assert_eq!(f.patched_size, 30_178_579);
    assert!(fd::applies_to(&fdb, &ff));
}

#[test]
fn core_xpak_table() {
    let Some(x) = cut("core.xpak") else { eprintln!("skipped: no cuts"); return };
    let h = kapi::header(&x).unwrap();
    assert_eq!((h.version, h.parts, h.rows, h.file_len), (0x10, 74, 423_722, 0x68_0000));
    let r = kapi::rows(&x).unwrap();
    assert_eq!(kapi::find(&r, r[1000].0), Some(r[1000].1));
    let parts: std::collections::HashSet<u64> = r.iter().map(|x| x.1 >> 56).collect();
    assert_eq!(parts.len(), 74);
}
