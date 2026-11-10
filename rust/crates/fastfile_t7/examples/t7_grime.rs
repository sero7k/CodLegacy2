//! A map's weather grime: the `volume_weathergrime` entities and the
//! `GfxWorld`'s grime volume records (120 bytes each, the six images at
//! +72): `t7_grime <map.ff>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let src = fastfile_t7::map::MapSources::beside(path);
    let map = fastfile_t7::map::T7Map::load(&src).expect("map");
    for b in map.entities.split('}') {
        if b.contains("weathergrime") {
            println!("{}\n--", b.trim());
        }
    }
    let zone = fastfile_t7::Zone::open(path).expect("zone");
    let w = fastfile_t7::walk::walk(&zone);
    let Some(g) = w.capture.world.gfx_worlds.first() else { return };
    let Some((at, n)) = g.grime_volumes else {
        println!("no grime volumes");
        return;
    };
    for i in 0..n {
        let r = &zone.image[at + 120 * i..at + 120 * (i + 1)];
        let f: Vec<String> = r[..72].chunks(4).map(|c| {
            let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            let u = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            if v.is_finite() && v.abs() < 1e7 && (v == 0.0 || v.abs() > 1e-6) { format!("{v}") } else { format!("{u:#x}") }
        }).collect();
        println!("volume {i}: {}", f.join(" "));
        println!("  images {:?}", g.grime_images[i]);
        println!("  tail {:02x?}", &r[112..120]);
    }
}
