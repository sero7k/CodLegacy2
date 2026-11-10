//! What collision holds a point: `t7_solid <map.ff> <x> <y> <z>` prints the
//! brushes whose planes all hold it (contents, plane count, bounds from
//! the axial planes) and the static models whose origin is within 256.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let p: [f32; 3] = [a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap()];
    let clip = map.clip.as_ref().expect("clip");
    println!("{} brushes, {} static models, {} tris", clip.brushes.len(), clip.static_models.len(), clip.tris.len());
    for (i, b) in clip.brushes.iter().enumerate() {
        let d = |pl: &[f32; 4]| pl[0] * p[0] + pl[1] * p[1] + pl[2] * p[2] - pl[3];
        let inside = b.planes.iter().all(|pl| d(pl) <= 0.0);
        let inside_neg = b.planes.iter().all(|pl| d(pl) >= 0.0);
        if inside || inside_neg {
            let ax: Vec<String> = b.planes.iter().filter(|pl| pl[0].abs() + pl[1].abs() + pl[2].abs() < 1.001 && (pl[0].abs() > 0.999 || pl[1].abs() > 0.999 || pl[2].abs() > 0.999)).map(|pl| format!("{:?}", pl.map(|v| v.round()))).collect();
            println!("brush {i}: contents {:#x} {} planes ({}) axial {}", b.contents, b.planes.len(), if inside { "n.p<=d" } else { "n.p>=d" }, ax.join(" "));
        }
    }
    // T7_BRUSH=<n>: who lists that brush, and its sides' surface flags.
    if let Some(n) = std::env::var("T7_BRUSH").ok().and_then(|v| v.parse::<u16>().ok()) {
        for (i, l) in clip.leafs.iter().enumerate() {
            if l.brushes.contains(&n) {
                println!("leaf {i} ({} brushes) lists brush {n}; bounds {:?} {:?}", l.brushes.len(), l.mins, l.maxs);
            }
        }
        for (i, m) in clip.models.iter().enumerate() {
            if m.leaf.brushes.contains(&n) {
                println!("model *{i} ({} brushes) lists brush {n}", m.leaf.brushes.len());
            }
        }
        let b = &clip.brushes[usize::from(n)];
        println!("brush {n}: contents {:#x}, side flags {:x?}", b.contents, b.plane_surface_flags);
        for (i, (name, sf, cf)) in clip.materials.iter().enumerate().take(400) {
            if name.contains("clip") || name.contains("sky") || name.contains("volume") || name.contains("bounds") {
                println!("material {i}: {name} surface {sf:#x} contents {cf:#x}");
            }
        }
    }
    for m in &clip.static_models {
        let d2: f32 = (0..3).map(|k| (m.origin[k] - p[k]).powi(2)).sum();
        if d2 < 256.0 * 256.0 {
            println!("static {} contents {:#x} at {:?} scale {}", m.name, m.contents, m.origin.map(|v| v.round()), m.scale);
        }
    }
}

#[allow(dead_code)]
fn _unused() {}
