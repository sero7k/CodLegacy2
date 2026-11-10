//! A map's volume decals (`GfxVolumeDecal`, 184 bytes): each record as
//! floats/words, its material, technique set and the technique slots'
//! arguments: `t7_voldecals <map.ff>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&zone);
    let c = &w.capture;
    let Some(g) = c.world.gfx_worlds.first() else { return };
    let Some((at, n)) = g.volume_decals else {
        println!("no volume decals");
        return;
    };
    println!("{n} volume decals at {at:#x}");
    let img = &zone.image;
    let mut sets = std::collections::BTreeSet::new();
    for i in 0..n {
        let r = &img[at + 184 * i..at + 184 * (i + 1)];
        let f: Vec<String> = r.chunks(4).map(|b| {
            let u = u32::from_le_bytes(b.try_into().unwrap());
            let x = f32::from_bits(u);
            if x.is_finite() && (x == 0.0 || (x.abs() > 1e-4 && x.abs() < 1e7)) { format!("{x}") } else { format!("{u:#x}") }
        }).collect();
        let m = g.volume_decal_materials.get(i).cloned().flatten();
        let ts = m.as_ref().and_then(|m| c.materials.iter().find(|r| &r.name == m)).and_then(|r| r.techset.clone());
        println!("[{i}] {m:?} [{ts:?}]\n    {}", f.join(" "));
        if let Some(t) = ts {
            sets.insert(t);
        }
    }
    for ts in sets {
        let Some(rec) = c.techsets.iter().find(|t| t.name == ts) else { continue };
        for (k, t) in rec.techniques.iter().enumerate() {
            let Some(t) = t else { continue };
            println!("{ts} slot {k}: programs {:?}", rec.programs[k]);
            if let Some(sm) = t.state_map {
                let w = |o: usize| u32::from_le_bytes(img[sm + o..sm + o + 4].try_into().unwrap());
                println!("   state blend0 {:#010x} depth {:#x} stencil {:#x} {:#x} raster {:#x} ref {}", w(0), w(0x20), w(0x24), w(0x28), w(0x2c), img[sm + 0x30]);
            }
            if let Some(d) = t.decl {
                println!("   decl {:02x?}", &img[d..d + 16]);
            }
            for a in &t.args {
                println!("   arg ty {:#06x} loc {} size {} buf {} u {:08x} {:08x}", a.ty, a.location, a.size, a.buffer, a.u[0], a.u[1]);
            }
        }
    }
}
