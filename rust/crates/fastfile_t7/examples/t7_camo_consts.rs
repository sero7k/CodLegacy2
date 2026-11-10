//! Every material with `camo` in its name in a Black Ops III zone: its
//! technique set, its pictures, and its named constants that move a camo
//! (flipbook `rowCount` / `columnCount` / `imageTime`, `scrollUVs`,
//! `rotateUVs`, `zoomUVs`, `flicker*`, `emissive*`, `hdrScale` ...). For
//! reading how Dark Matter, Diamond, Gold and the other camos animate.
//!
//! `cargo run -p fastfile_t7 --example t7_camo_consts -- <core_common.ff> [name part] > camos.txt`
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: t7_camo_consts <zone.ff> [material name part]");
        std::process::exit(2);
    };
    let part = args.next().unwrap_or_else(|| "camo".to_owned()).to_ascii_lowercase();
    let zone = fastfile_t7::Zone::open(Path::new(&path)).expect("zone");
    let walk = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options::default());
    let cap = &walk.capture;
    println!("walked {} assets; {} materials", walk.walked.len(), cap.materials.len());
    for m in cap.materials.iter().filter(|m| m.name.to_ascii_lowercase().contains(&part)) {
        println!("\n== {} [{}]", m.name, m.techset.as_deref().unwrap_or("?"));
        for (hash, image) in &m.textures {
            println!("  texture {hash:08x} {image}");
        }
        let mut consts: Vec<_> = fastfile_t7::map::zone_material_constants(cap, &zone.image, m).into_iter().collect();
        consts.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, v) in consts {
            if v.iter().any(|x| *x != 0.0) {
                println!("  {name} = {v:?}");
            }
        }
    }
}
