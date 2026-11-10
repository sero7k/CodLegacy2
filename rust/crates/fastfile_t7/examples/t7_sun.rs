//! A map's sun as BO3's renderer takes it: `t7_sun <map.ff>` prints each sun
//! volume's four lighting states (`fastfile_t7::native::view_sun`), then
//! the vukko lists and `skyMx`/`sunMx` with the eye at the origin looking
//! down +x (`native::sky_matrices`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&zone);
    for g in &w.capture.world.gfx_worlds {
        for (i, &v) in g.sun_volumes.iter().enumerate() {
            for s in 0..4 {
                let Some(set) = fastfile_t7::native::global_light_settings(&zone.image, v, s) else { continue };
                println!("volume {i} state {s}: {:?}", fastfile_t7::native::view_sun(&set));
            }
        }
        println!("vukko: modifiers {:?}, boxes {:?}", g.vukko_mods, g.vukko_boxes);
        let (m, used) = fastfile_t7::native::sky_matrices(&zone.image, g.header_at, 0, 0, g.vukko_mods, g.vukko_boxes, [0.0; 3], [1.0, 0.0, 0.0], 0.0);
        println!("vukko records used {used:?}");
        for (n, r) in ["skyMxR", "skyMxG", "skyMxB", "sunMxR", "sunMxG", "sunMxB"].iter().zip(m) {
            println!("{n} {r:?}");
        }
    }
}
