//! bo2mp: the fog the sky program gets at an eye: `t7_sky_fog <map.ff> x y z`
//! prints the 56 `fogConstants` floats (LightingGlobals 0x30..) as the
//! native lighting fills them (bank at the eye, client fog, evaluation,
//! config), for checking a map's sky against its fog offline.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let eye: [f32; 3] = [a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap()];
    let w = fastfile_t7::walk::walk(&zone);
    let g = w.capture.world.gfx_worlds.first().expect("world");
    let img = &zone.image;
    let light = g.sun_volumes.first().and_then(|&v| fastfile_t7::native::global_light_settings(img, v, 0));
    let src = fastfile_t7::fog::FogSources { world_header: g.header_at, fog_volumes: g.fog_volumes, fog_planes: g.fog_planes, modifiers: g.fog_modifiers, modifier_planes: g.fog_modifier_planes };
    let mut t = fastfile_t7::fog::FogTransitions::default();
    let (bank, scales) = t.update(img, &src, 0, 1, eye, 0, true);
    println!("bank {bank:?}");
    println!("scales {scales:?}");
    let client = fastfile_t7::fog::client_fog(&bank, scales, light.as_ref());
    println!("client {client:?}");
    let o = fastfile_t7::fog::evaluate(&client, eye);
    let k = fastfile_t7::fog::fog_constants(&fastfile_t7::fog::config(&o, eye, true));
    println!("fog {}", k.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(" "));
    // (`T7_SKY_SCALE=1`: also the sky material's `skyScale` as the light
    // bake reads it, through the whole map load.)
    if std::env::var("T7_SKY_SCALE").is_ok() {
        let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
        if let Some(l) = map.light_refs() {
            println!("sky material {} skyScale {}", l.sky_material, l.sky_scale);
        }
    }
}
