//! The zone's effect light elements (type 8), their light descriptions and
//! the scene light each adds at the start of its life (`scene_light`):
//! `t7_fx_lights <map.ff>`.
use fastfile_t7::scene_light as sl;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let src = fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]));
    let map = fastfile_t7::map::T7Map::load(&src).expect("map");
    let (mut elems, mut found) = (0, 0);
    for fx in &w.capture.fx {
        for (i, e) in fx.elems.iter().enumerate() {
            if e.raw.get(0xc8) != Some(&8) {
                continue;
            }
            elems += 1;
            let name = e.visuals.first().cloned().unwrap_or_default();
            let Some(d) = map.light_description(&name) else {
                println!("{} #{i}: {name} (no description)", fx.name);
                continue;
            };
            found += 1;
            if std::env::var("DUMP").is_ok() { dump(d); }
            let ty = u32::from_le_bytes(d[0x48..0x4c].try_into().unwrap());
            let s = &e.samples[2];
            let word = |w: usize| sl::sample_word(s, 0, 0.0, w, 0.5).unwrap_or(f32::NAN);
            let colour = sl::sample_colour(s, 0, 0.0, 0.5).unwrap_or([0; 3]);
            let el = sl::ElemLight { origin: [0.0; 3], axis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], colour, intensity: word(0x18), size: word(0x1c), angle: word(0x20) };
            let l = sl::elem_light(d, &el);
            let r = l.as_ref().map(|l| f32::from_le_bytes(l[0x14c..0x150].try_into().unwrap()));
            println!("{} #{i}: {name} type {ty} colour {colour:?} intensity {:.2} size {:.1} angle {:.1} -> radius {r:?}", fx.name, el.intensity, el.size, el.angle);
        }
    }
    println!("{elems} light elements, {found} with their light description");
}

#[allow(dead_code)]
pub fn dump(d: &[u8]) {
    for (i, c) in d.chunks(16).enumerate() {
        let w: Vec<String> = c.chunks(4).map(|b| { let v = u32::from_le_bytes(b.try_into().unwrap()); let f = f32::from_bits(v); if v != 0 && f.abs() > 1e-6 && f.abs() < 1e7 { format!("{f:.3}") } else { format!("{v:#x}") } }).collect();
        println!("  {:03x}: {}", i * 16, w.join(" "));
    }
}
