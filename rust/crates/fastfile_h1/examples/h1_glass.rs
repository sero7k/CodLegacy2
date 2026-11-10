//! The intact glass of a map: definitions and a few pieces with their
//! world bounds.
//!
//! `cargo run -p fastfile_h1 --release --example h1_glass -- <map.ff>`
fn f(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn main() {
    let p = std::env::args().nth(1).expect("zone");
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for g in &cap.fx_glass {
        println!("fxworld {}: {} defs, {} pieces, {} geo words", g.name, g.defs.len(), g.init_piece_count, g.init_geo_count);
        for (i, d) in g.defs.iter().enumerate() {
            let nm = |m: Option<fastfile_h1::walk::AssetId>| m.map(|m| cap.name_of(m).to_string()).unwrap_or_default();
            println!("  def {i}: {} / {} half {} opaque {} color {:?} tex {:?}", nm(d.material), nm(d.material_shattered), d.half_thickness, d.is_opaque, d.color, d.tex_vecs);
        }
        let (Some(ps), Some(gs)) = (g.init_pieces, g.init_geo) else { continue };
        let (ps, gs) = (ps.get(&z.image), gs.get(&z.image));
        let mut at = 0usize;
        for i in 0..g.init_piece_count {
            let s = &ps[i * 56..i * 56 + 56];
            let (vc, fc, def) = (s[51] as usize, s[52] as usize, s[50]);
            if i < 6 {
                let q = [f(s, 0), f(s, 4), f(s, 8), f(s, 12)];
                let o = [f(s, 16), f(s, 20), f(s, 24)];
                let vs: Vec<[i16; 2]> = (0..vc).map(|k| { let w = &gs[(at + k) * 4..(at + k) * 4 + 4]; [i16::from_le_bytes([w[0], w[1]]), i16::from_le_bytes([w[2], w[3]])] }).collect();
                println!("  piece {i}: def {def} verts {vc} fan {fc} quat {q:?} origin {o:?} radius {} verts {:?}", f(s, 28), &vs[..vs.len().min(6)]);
            }
            at += vc + fc;
        }
        println!("  geo words used {at} of {}", g.init_geo_count);
    }
}
