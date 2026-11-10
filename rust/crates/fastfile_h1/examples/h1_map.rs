//! Prints each map zone's world summary: `cargo run -p fastfile_h1
//! --release --example h1_map -- <zone.ff>...` (or the zones in `H1_WALK`,
//! `:`-separated). With `--pixels`, also reads every streamed image from its
//! pak (`<zone>.pak` for index 96, `imagefile<N>.pak` beside the zone or in
//! `H1_IMAGEFILES` for the others) and counts what reads.

use std::path::PathBuf;

fn main() {
    let mut pixels = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    for a in std::env::args_os().skip(1) {
        if a == "--pixels" {
            pixels = true;
        } else {
            paths.push(a.into());
        }
    }
    if paths.is_empty()
        && let Some(v) = std::env::var_os("H1_WALK")
    {
        paths = std::env::split_paths(&v).collect();
    }
    let retail = std::env::var_os("H1_IMAGEFILES").map(PathBuf::from);
    for path in paths {
        let zone = match fastfile_h1::Zone::open(&path) {
            Ok(z) => z,
            Err(e) => {
                println!("{}: {e}", path.display());
                continue;
            }
        };
        let (map, cap) = fastfile_h1::map::IwMap::load(&zone);
        let s = fastfile_h1::map::summary(&map, &cap);
        println!(
            "{}: surfaces {} vertices {} indices {} static models {} brush models {} brushes {} collision tris {} entity chars {} spawns {} lightmaps {} images inline {} streamed {} (own pak {})",
            path.display(),
            s.surfaces,
            s.vertices,
            s.indices,
            s.static_models,
            s.brush_models,
            s.brushes,
            s.collision_tris,
            s.entity_chars,
            s.spawns,
            s.lightmaps,
            s.images_inline,
            s.images_streamed,
            s.images_in_zone_pak,
        );
        if let Some(sun) = map.sun {
            println!("  sun dir {:?} color {:?}", sun.dir, sun.color);
        }
        if let Some(clip) = &map.clip {
            for (i, st) in clip.stages.iter().enumerate() {
                let hulls = clip
                    .stage_triggers
                    .get(usize::from(st.trigger_index))
                    .map_or(0, Vec::len);
                println!(
                    "  stage {i}: origin {:?} trigger {} ({hulls} hulls) sun light {} radiometric scale {}",
                    st.origin, st.trigger_index, st.sun_primary_light_index, st.radiometric_scale
                );
            }
        }
        for n in &map.notes {
            println!("  note: {n}");
        }
        if pixels {
            let mut paks = fastfile_h1::image::ImagePaks::for_zone(&path, retail.as_deref());
            let (mut ok, mut failed) = (0usize, std::collections::BTreeMap::<u16, usize>::new());
            for rec in cap.images.iter().filter(|i| i.streamed) {
                match fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, 1 << 14) {
                    Ok(_) => ok += 1,
                    Err(_) => {
                        let idx = rec
                            .stream_files
                            .iter()
                            .flatten()
                            .next()
                            .map_or(0, |s| s.file_index);
                        *failed.entry(idx).or_insert(0) += 1;
                    }
                }
            }
            println!("  streamed pixels read {ok}; not read, by file index: {failed:?}");
        }
    }
}
