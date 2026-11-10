//! `t7_exposure_check <map.ff> [spawns]`: CPU only. The eye exposure the
//! post chain gives a view at each of the map's team deathmatch spawns
//! (`post_exposure`: the sun volume's state-0 base, the exposure volumes
//! the eye is in, the exposure grids metered along the view, settled as a
//! cut), against the sun: the metered luminance, the EV and the scale the
//! lit scene is divided by. A view the grids do not cover meters the sun
//! volume's default luminance.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let n: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(12);
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&zone)).expect("map");
    let (img, cap) = map.zone_data().expect("zone data");
    let world = cap.world.gfx_worlds.first().expect("world");
    let sun_at = *world.sun_volumes.first().expect("sun volume");
    let base = img.get(sun_at..sun_at + 0xbc).and_then(render_t7::post_exposure::ExposureBase::from_settings).expect("exposure base");
    let volumes: Vec<render_t7::post_exposure::ExposureVolume> = world.exposure_volumes.map_or(Vec::new(), |(at, n)| (0..n).filter_map(|i| img.get(at + 0x6c * i..at + 0x6c * (i + 1)).and_then(render_t7::post_exposure::ExposureVolume::parse)).collect());
    let planes: Vec<[f32; 4]> = world.exposure_planes.map_or(Vec::new(), |(at, n)| (0..n).map(|i| std::array::from_fn(|k| img.get(at + 16 * i + 4 * k..at + 16 * i + 4 * k + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap())))).collect());
    let lighting = world.sun_lighting.first();
    let grids_span = lighting.and_then(|l| l.exposure_grids.first().copied().flatten());
    let grids = render_t7::post_exposure::ExposureGrids::new(&img, sun_at, 0, grids_span, lighting.and_then(|l| l.exposure_grid_parts.first()).map_or(&[][..], Vec::as_slice));
    let settings = fastfile_t7::native::global_light_settings(&img, sun_at, 0).expect("light settings");
    let sun = fastfile_t7::native::view_sun(&settings);
    println!("{}: exposure base {base:?}; {} volumes; grids {} probes, default luminance {}; sun colour x intensity {:?}, direction {:?}", zone.display(), volumes.len(), grids_span.map_or(0, |g| g.1), grids.default, sun.colour, sun.direction);
    // The vision rawfiles the map and the shared zones hold (`T7_VISIONS=1`).
    if std::env::var("T7_VISIONS").is_ok() {
        let dir = zone.parent().unwrap();
        let mut list: Vec<(String, std::sync::Arc<Vec<u8>>)> = vec![("map".into(), img.clone())];
        for z in ["core_common", "mp_common", "core_patch", "mp_patch"] {
            if let Some(w) = fastfile_t7::shared::zone(dir, z) {
                list.push((z.into(), std::sync::Arc::new(w.image.to_vec())));
            }
        }
        for (z, im) in &list {
            let mut names = std::collections::BTreeSet::new();
            let key = b".vision\0";
            let mut from = 0;
            while let Some(p) = im[from..].windows(key.len()).position(|w| w == key) {
                let end = from + p + 7;
                let start = im[..end].iter().rposition(|&b| !(0x21..0x7f).contains(&b)).map_or(0, |i| i + 1);
                names.insert(String::from_utf8_lossy(&im[start..end]).into_owned());
                from = end + 1;
            }
            println!("visions in {z}: {names:?}");
        }
    }
    let mut seen = 0;
    for block in map.entities.split('}') {
        if !block.contains("mp_tdm_spawn") || seen >= n {
            continue;
        }
        let val = |k: &str| block.split('\n').find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
        let (Some(o), yaw) = (val("origin"), val("angles").and_then(|a| a.split_whitespace().nth(1).and_then(|y| y.parse::<f32>().ok())).unwrap_or(0.0)) else { continue };
        let v: Vec<f32> = o.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() != 3 {
            continue;
        }
        seen += 1;
        let eye = [v[0], v[1], v[2] + 60.0];
        let y = yaw.to_radians();
        let dir = [y.cos(), y.sin(), 0.0];
        let inside = render_t7::post_exposure::volume_weights(eye, &volumes, &planes, 1 << 4);
        let metered = grids.meter(eye, dir);
        let e = render_t7::post_exposure::ExposureState::default().update(base, &volumes, &inside, 0, metered, false);
        println!("  eye {eye:?} yaw {yaw:.0}: metered {metered:.3}, volumes {inside:?}, EV {:.2} (min {:.2} max {:.2} bias {:.2}), lit scene x {:.6}", e.ev, e.min_ev, e.max_ev, e.bias, 1.0 / e.scale);
    }
}
