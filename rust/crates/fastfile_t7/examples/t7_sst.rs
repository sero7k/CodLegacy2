//! `t7_sst <map.ff>`: the map's sun shadow tree (state 0) as the zone
//! records it: the record's words, the per-mip tables, the min/max tree,
//! and whether its streamed data reads from the packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let Some(t) = map.light_refs().and_then(|r| r.shadow_tree.clone()) else {
        println!("no shadow tree");
        return;
    };
    println!("key {:#x}", t.key);
    for (i, c) in t.record.chunks(16).enumerate() {
        let w: Vec<String> = c.chunks(4).map(|b| format!("{:08x}", u32::from_le_bytes(b.try_into().unwrap()))).collect();
        let f: Vec<String> = c.chunks(4).map(|b| format!("{:.4}", f32::from_le_bytes(b.try_into().unwrap()))).collect();
        println!("  +{:#04x} {}   {}", i * 16, w.join(" "), f.join(" "));
    }
    for (i, c) in t.info.chunks(16).enumerate() {
        let w: Vec<String> = c.chunks(4).map(|b| format!("{:08x}", u32::from_le_bytes(b.try_into().unwrap()))).collect();
        let f: Vec<String> = c.chunks(4).map(|b| format!("{:.4}", f32::from_le_bytes(b.try_into().unwrap()))).collect();
        println!("  info +{:#04x} {}   {}", i * 16, w.join(" "), f.join(" "));
    }
    println!("minmax image {:?}", t.minmax_image);
    println!("mip table a {:?}", t.mip_a);
    println!("mip table b {:?}", t.mip_b);
    for (i, c) in t.minmax_record.chunks(16).enumerate() {
        let w: Vec<String> = c.chunks(4).map(|b| format!("{:08x}", u32::from_le_bytes(b.try_into().unwrap()))).collect();
        println!("  minmax +{:#04x} {}", i * 16, w.join(" "));
    }
    println!("minmax u16s: {} (first {:?})", t.minmax.len(), &t.minmax[..t.minmax.len().min(16)]);
    match map.paks().map(|p| p.read(t.key)) {
        Some(Ok(d)) => {
            println!("streamed data: {} bytes", d.len());
            for (i, c) in d[..256.min(d.len())].chunks(32).enumerate() {
                let w: Vec<String> = c.chunks(4).map(|b| format!("{:08x}", u32::from_le_bytes(b.try_into().unwrap()))).collect();
                println!("  @{:#05x} {}", i * 32, w.join(" "));
            }
        }
        Some(Err(e)) => println!("streamed data: {e}"),
        None => println!("no packages"),
    }
}
