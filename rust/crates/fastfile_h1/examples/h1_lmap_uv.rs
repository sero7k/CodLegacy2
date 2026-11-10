//! World vertices' lightmap coordinates: range, and per lightmap page the
//! coordinate spread, to see whether they are usable as 0..1 page uvs.
use std::collections::BTreeMap;
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (map, _) = fastfile_h1::map::IwMap::load(&zone);
    let w = map.world.expect("world");
    println!("{} vertices, {} surfaces", w.vertices.len(), w.surfaces.len());
    let mut by: BTreeMap<u8, (usize, f32, f32, f32, f32, usize)> = BTreeMap::new();
    for s in &w.surfaces {
        let e = by.entry(s.lightmap_index).or_insert((0, 9.0, -9.0, 9.0, -9.0, 0));
        let n = usize::from(s.vertex_count);
        for k in 0..n {
            let Some(v) = w.vertices.get(s.first_vertex as usize + k) else { e.5 += 1; continue };
            let [u, t] = v.lmap_coord;
            e.0 += 1;
            e.1 = e.1.min(u);
            e.2 = e.2.max(u);
            e.3 = e.3.min(t);
            e.4 = e.4.max(t);
        }
    }
    for (k, v) in by {
        println!("page {k}: {} verts, u {:.3}..{:.3}, v {:.3}..{:.3}, {} out of range", v.0, v.1, v.2, v.3, v.4, v.5);
    }
    // a few raw samples
    for v in w.vertices.iter().step_by(w.vertices.len() / 8 + 1).take(8) {
        println!("sample tex {:?} lmap {:?}", v.tex_coord, v.lmap_coord);
    }
}
