//! Prints what a Ghosts map's sky is made of: the `GfxSky` pictures and
//! their paks, the pictures the sky surfaces' materials reference, the
//! worldspawn keys, and the sun.
//!
//! `cargo run -p render_iw6 --example iw6_sky -- <map.ff>`

fn main() {
    let p = std::env::args().nth(1).expect("map.ff");
    let z = fastfile_iw6::Zone::open(std::path::Path::new(&p)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&z);
    let map = fastfile_iw6::map::read_map(&cap, &z.image, &z.assets.script_strings);
    let w = map.world.as_ref().expect("world");
    let describe = |r: &fastfile_iw6::walk::ImageRec| {
        format!(
            "{} map_type {} DXGI {} {}x{} levels {} streamed {} paks {:?} streams {:?}",
            r.name,
            r.map_type,
            r.format,
            r.width,
            r.height,
            r.level_count,
            r.streamed,
            r.stream_files.iter().flatten().map(|f| f.file_index).collect::<Vec<_>>(),
            r.streams
        )
    };
    println!("sky surfaces: {} in {} skies", w.sky_surfaces.len(), w.sky_images.len());
    for i in w.sky_images.iter().flatten() {
        println!("GfxSky image: {}", describe(&cap.images[i.index as usize]));
    }
    let mut seen = std::collections::BTreeSet::new();
    for &s in &w.sky_surfaces {
        let Some(m) = w.surfaces.get(s as usize).and_then(|s| s.material) else { continue };
        let name = cap.name_of(m).to_owned();
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(mr) = cap.materials.iter().find(|x| x.name.eq_ignore_ascii_case(&name)) else {
            println!("sky material {name}: not in this zone");
            continue;
        };
        println!(
            "sky material {name}: techset {:?} sort {} state_flags {:#x} camera_region {}",
            mr.techset.map(|t| cap.name_of(t).to_owned()),
            mr.sort_key,
            mr.state_flags,
            mr.camera_region
        );
        for t in &mr.textures {
            println!(
                "   texture semantic {} sampler {:#x}: {:?}",
                t.semantic,
                t.sampler_state,
                t.image.map(|i| describe(&cap.images[i.index as usize]))
            );
        }
    }
    println!("sky_surfaces {:?} ranges {:?} surfaces {}", w.sky_surfaces, w.ranges, w.surfaces.len());
    for k in [2646usize, 2647, 2648, 15497, 15498, 15499] {
        if let Some(sf) = w.surfaces.get(k) {
            println!("  surface {k}: {:?} verts {} flags {:#x} mins {:?} maxs {:?}", sf.material.map(|m| cap.name_of(m).to_owned()), sf.vertex_count, sf.flags, sf.mins, sf.maxs);
        }
    }
    if let Some(g) = cap.gfx_worlds.first() {
        println!("gfx world sorted_surfs {:?}", g.sorted_surfs);
        if let Some(at) = g.sorted_surfs {
            let rd = |i: usize| u32::from_le_bytes(z.image[at + 4 * i..at + 4 * i + 4].try_into().unwrap());
            println!("sorted[2646..2649] = {:?}; position of 15498 in sorted: {:?}", (2646..2649).map(rd).collect::<Vec<_>>(), (0..18001usize).find(|&i| rd(i) == 15498));
        }
    }
    let mut skymats = std::collections::BTreeMap::<String, usize>::new();
    for (i, sf) in w.surfaces.iter().enumerate() {
        if let Some(m) = sf.material {
            let n = cap.name_of(m).to_owned();
            if n.to_ascii_lowercase().contains("sky") {
                *skymats.entry(format!("{n} @surface {i}")).or_default() += 1;
                if i < 4 || skymats.len() < 3 {}
            }
        }
    }
    println!("surfaces whose material name has 'sky': {skymats:?}");
    for m in cap.materials.iter().filter(|m| m.name.to_ascii_lowercase().contains("sky")) {
        println!("material {} techset {:?} textures {:?}", m.name, m.techset.map(|t| cap.name_of(t).to_owned()), m.textures.iter().map(|t| t.image.map(|i| cap.images[i.index as usize].name.clone())).collect::<Vec<_>>());
    }
    for i in cap.images.iter().filter(|i| i.map_type == 5 || i.name.to_ascii_lowercase().contains("sky")) {
        println!("image {}", describe(i));
    }
    println!("sun {:?}", map.sun);
    for l in map.entity_string().lines().take(60) {
        println!("ent {l}");
    }
}
