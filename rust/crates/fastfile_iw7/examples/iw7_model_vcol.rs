//! A model's vertex colour bytes (memory order) per surface: `iw7_model_vcol <map.ff> <name part>`.
use fastfile_iw7::map::{Iw7Map, MapSources};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = Iw7Map::load(&MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    for m in map.models.iter().filter(|m| m.name.contains(a[2].as_str())) {
        for (i, s) in m.surfaces.iter().enumerate() {
            let n = s.verts.len().max(1) as f64;
            let mut mean = [0f64; 4];
            for v in &s.verts {
                for c in 0..4 {
                    mean[c] += f64::from(v[16 + c]);
                }
            }
            println!("{} surf {i}: {} verts, colour mean {:?}", m.name, s.verts.len(), mean.map(|x| (x / n).round()));
        }
    }
}
