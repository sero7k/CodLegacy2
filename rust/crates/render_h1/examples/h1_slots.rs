//! Each technique set's filled slots and their techniques (the slot
//! numbers of `MaterialTechniqueType`), for the techsets whose name holds a
//! pattern, or a summary of which technique names fill each slot.
//!
//! `cargo run -p render_h1 --example h1_slots -- <pattern|--summary> <zone.ff>...`

use render_h1::pool::Pool;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a[0] == "--cull" {
        // The sun shadow's per-cascade caster culling, headless: from the
        // map's intermission point, looking along each of four headings,
        // the shadow casters each cascade keeps.
        use render_h1::sun_shadow as ss;
        let zones: Vec<std::path::PathBuf> = a[1..].iter().map(Into::into).collect();
        let scene = render_h1::scene::Scene::load(&zones).expect("load");
        let bounds = render_h1::world::draw_bounds(&scene);
        let casters: Vec<usize> = (0..scene.draws.len())
            .filter(|&i| {
                render_h1::world::pick(
                    &scene.pool,
                    &scene.materials[scene.draws[i].material].techset,
                    &[render_h1::world::slot::SHADOW_CASTER],
                )
                .is_some()
            })
            .collect();
        let eye = scene.intermission().map_or(glam::Vec3::ZERO, |p| p.0);
        println!(
            "{} draws, {} casters ({} with bounds), eye {eye}, sun {}",
            scene.draws.len(),
            casters.len(),
            casters.iter().filter(|&&i| bounds[i].is_some()).count(),
            scene.sun.dir
        );
        let settings = ss::Settings::from_lightset(&scene.lightset);
        for yaw in [0.0f32, 90.0, 180.0, 270.0] {
            let (s, c) = yaw.to_radians().sin_cos();
            let forward = glam::Vec3::new(c, s, -0.1).normalize();
            let right = forward.cross(glam::Vec3::Z).normalize();
            let view = ss::View {
                eye,
                forward,
                right,
                up: right.cross(forward),
                tan_x: 1.0,
                tan_y: 0.5625,
                near: 4.0,
            };
            let setup = ss::Setup::new(settings, scene.sun.dir, view, ss::caster_bounds(&scene));
            let kept: Vec<usize> = (0..setup.cascades())
                .map(|k| {
                    let p = setup.caster_planes(k);
                    casters
                        .iter()
                        .filter(|&&i| bounds[i].is_none_or(|(lo, hi)| ss::box_inside(&p, lo, hi)))
                        .count()
                })
                .collect();
            println!(
                "yaw {yaw:3}: split {:.1}, kept per cascade {kept:?}, tiles {}",
                setup.fit.depth,
                setup.tile_keys().len()
            );
        }
        return;
    }
    if a[0] == "--lightset" {
        // The light set's sun shadow keys.
        let zones: Vec<std::path::PathBuf> = a[1..].iter().map(Into::into).collect();
        let scene = render_h1::scene::Scene::load(&zones).expect("load");
        for k in [
            "sunshadowscale",
            "usedsuncascadecount",
            "sunfilterradius",
            "sunsamplesizenear",
            "spotfilterradius",
            "veil",
            "veilstrength",
            "veilbackgroundstrength",
        ] {
            println!("{k} = {}", scene.lightset.f(k, f32::NAN));
        }
        let mut keys: Vec<_> = scene.lightset.values.keys().collect();
        keys.sort();
        println!("{} keys: {keys:?}", keys.len());
        return;
    }
    if a[0] == "--states" {
        // The map's materials' fixed-function state per slot given (a[1]),
        // counted: (cull, depth write, depth test, polygon offset).
        let slot: usize = a[1].parse().expect("slot");
        let zones: Vec<std::path::PathBuf> = a[2..].iter().map(Into::into).collect();
        let scene = render_h1::scene::Scene::load(&zones).expect("load");
        let mut n: BTreeMap<String, usize> = BTreeMap::new();
        for m in &scene.materials {
            let e = m.state_entry.get(slot).copied().unwrap_or(255);
            let k = if e == 255 {
                "none".to_string()
            } else {
                let s = m.state(slot, 0);
                format!(
                    "cull {} write {} test {} offset {} mask {:#x}",
                    s.cull, s.depth_write, s.depth_test, s.polygon_offset, s.write_mask
                )
            };
            *n.entry(k).or_default() += 1;
        }
        for (k, c) in n {
            println!("{c:5} {k}");
        }
        return;
    }
    let mut pool = Pool::default();
    for p in &a[1..] {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(p)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        pool.add(&cap, &zone.image);
    }
    if a[0] == "--summary" {
        // slot -> (count, a few names)
        let mut by: BTreeMap<usize, (usize, Vec<String>)> = BTreeMap::new();
        for slots in pool.techsets.values() {
            for (s, n) in slots {
                let e = by.entry(*s).or_default();
                e.0 += 1;
                if e.1.len() < 4 && !e.1.contains(n) {
                    e.1.push(n.clone());
                }
            }
        }
        for (s, (n, names)) in by {
            println!("slot {s:3}: {n:6} {names:?}");
        }
        return;
    }
    let mut names: Vec<_> = pool.techsets.keys().filter(|k| k.contains(&a[0])).collect();
    names.sort();
    for k in names.into_iter().take(20) {
        println!("{k}");
        for (s, n) in &pool.techsets[k] {
            println!("  {s:3} {n}");
        }
    }
}
