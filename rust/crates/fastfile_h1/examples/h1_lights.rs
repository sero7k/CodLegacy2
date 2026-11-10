//! A map's primary lights and light environments, and how many world
//! surfaces and placed models use each environment:
//! `cargo run -p fastfile_h1 --release --example h1_lights -- <mp_map.ff> [--all]`.

use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(&args[0]);
    let all = args.iter().any(|a| a == "--all");
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (map, _cap) = fastfile_h1::map::IwMap::load(&zone);
    let lights = &map.primary_lights;
    let mut by_type: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    for l in lights {
        *by_type.entry((l.ty, l.can_use_shadow_map)).or_default() += 1;
    }
    println!("{} primary lights, (type, canUseShadowMap) counts {by_type:?}", lights.len());
    for (i, l) in lights.iter().enumerate() {
        if all || i < 12 {
            println!(
                "  {i:4} type {} shadow {} pb {} exp {} state {} colour {:?} origin {:?} dir {:?} radius {} cos out/in {}/{} fade {:?} bulb {} {:?}",
                l.ty,
                l.can_use_shadow_map,
                l.physically_based,
                l.exponent,
                l.lighting_state,
                l.color,
                l.origin,
                l.dir,
                l.radius,
                l.cos_half_fov_outer,
                l.cos_half_fov_inner,
                l.fade_offset,
                l.bulb_radius,
                l.bulb_length
            );
        }
    }
    let world = map.world.as_ref().expect("world");
    let mut surf: BTreeMap<u16, usize> = BTreeMap::new();
    for s in &world.surfaces {
        *surf.entry(u16::from(s.primary_light_env)).or_default() += 1;
    }
    let mut models: BTreeMap<u16, usize> = BTreeMap::new();
    for m in &world.static_models {
        *models.entry(m.primary_light_env).or_default() += 1;
    }
    println!("{} environments", map.primary_light_envs.len());
    let mut shapes: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for (i, e) in map.primary_light_envs.iter().enumerate() {
        let i = i as u16;
        let shape: Vec<String> = e
            .iter()
            .map(|&k| match lights.get(k as usize) {
                Some(l) if l.ty == 1 => "sun".to_string(),
                Some(l) if l.ty == 2 => format!("spot{}", if l.can_use_shadow_map != 0 { "S" } else { "" }),
                Some(l) if l.ty == 3 => "omni".to_string(),
                Some(l) => format!("t{}", l.ty),
                None => "?".into(),
            })
            .collect();
        let s = shapes.entry(shape.join("+")).or_default();
        s.0 += 1;
        s.1 += surf.get(&i).copied().unwrap_or(0);
        s.2 += models.get(&i).copied().unwrap_or(0);
        if all {
            println!(
                "  env {i}: {e:?} {shape:?} surfaces {} models {}",
                surf.get(&i).copied().unwrap_or(0),
                models.get(&i).copied().unwrap_or(0)
            );
        }
    }
    println!("environment shapes (envs, world surfaces, placed models):");
    for (k, v) in shapes {
        println!("  {:24} {:?}", if k.is_empty() { "(none)" } else { &k }, v);
    }
}
