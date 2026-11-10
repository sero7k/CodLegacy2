//! The world zones (`gfx_map_trzone`) a map zone holds, by their name hash,
//! against names the map might go by: `jup_trzones <install> <zone> [name...]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let oodle = fastfile_jup::oodle::load(std::path::Path::new(&a[1])).expect("oodle");
    let path = std::path::Path::new(&a[1]).join("zone").join(format!("{}.ff", a[2]));
    let z = fastfile_jup::map::read_image(&path, oodle.as_ref()).expect("zone");
    let b = &z.bytes;
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let follow = 0xffff_ffff_ffff_fffeu64;
    let mut names: Vec<String> = a[3..].to_vec();
    let stem = a[2].clone();
    for n in [
        stem.clone(),
        stem.trim_start_matches("mp_jup_").to_owned(),
        format!("mp_{}", stem.trim_start_matches("mp_jup_")),
        "mp_shipment".into(),
        "mp_m_cargo".into(),
        "mp_jup_shipment".into(),
        "mp_shipment_iw9".into(),
        "shipment".into(),
        "mp_m_shipment".into(),
        "mp_shipment2".into(),
        "mp_jup_shipment_iw9_tr".into(),
    ] {
        names.push(n);
    }
    let known: Vec<(u64, String)> = names.iter().map(|n| (fastfile_jup::map::name_hash(n), n.clone())).collect();
    let mut found = 0;
    let mut at = 0;
    while at + 0x48 <= b.len() {
        if u32_at(at + 0x18) != 0 && u32_at(at + 0x20) != 0 && (1..=4).all(|k| u64_at(at + 0x20 + 8 * k) == follow) {
            let h = u64_at(at);
            let n = known.iter().find(|(k, _)| *k == h).map_or("?", |(_, n)| n.as_str());
            println!("trzone at {at:#x}: hash {h:#018x} ({n}) vertex bytes {} list {} tables {} packed {}", u32_at(at + 0x18), u32_at(at + 0x1c), u32_at(at + 0x20), u32_at(at + 0x24));
            found += 1;
        }
        at += 4;
    }
    println!("{found} candidates; zone {} bytes", b.len());
    for (h, n) in &known {
        println!("  {n}: {h:#018x}");
    }
}
