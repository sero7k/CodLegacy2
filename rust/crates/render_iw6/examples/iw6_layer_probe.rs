//! Finds where a world vertex format's layer-stream components sit by data:
//! for every byte offset of the stride, how coherent the byte is between
//! neighbouring vertices of a surface (a real blend weight or layer texture
//! coordinate varies smoothly; a byte of another field does not). Prints,
//! per format, the declared layout and the coherence per offset.
//!
//! `cargo run -p render_iw6 --example iw6_layer_probe -- <map.ff>`
use std::collections::{BTreeMap, HashMap};

use render_iw6::scene::{List, Scene};

fn main() {
    let p: std::path::PathBuf = std::env::args().nth(1).expect("map.ff").into();
    let zones = Scene::zones_for(&p);
    let scene = Scene::load(&zones).expect("load");
    let mut layouts = render_iw6::layers::layouts(&scene.pool);
    let changed = render_iw6::layers::with_strides(&mut layouts, &scene.layer_strides);
    println!("stride changes {changed:?}");
    let mesh = &scene.meshes[0];
    // per format: per offset (sum of neighbour diffs, sum of random diffs, n)
    let mut acc: BTreeMap<u8, (u32, Vec<(f64, f64, u64)>, usize, Vec<String>)> = BTreeMap::new();
    let _ = List::Sky;
    for d in &scene.draws {
        if d.mesh != 0 || d.object.is_some() {
            continue;
        }
        let Some(layer) = d.layer else { continue };
        let m = &scene.materials[d.material];
        let Some(&f) = scene.pool.world_vert_format.get(&m.techset) else { continue };
        let Some(&stride) = scene.layer_strides.get(&f) else { continue };
        let stride = stride as usize;
        let e = acc.entry(f).or_insert_with(|| (stride as u32, vec![(0.0, 0.0, 0); stride], 0, Vec::new()));
        e.2 += 1;
        if e.3.len() < 3 && !e.3.contains(&m.techset) {
            e.3.push(m.techset.clone());
        }
        let first = d.first_index as usize;
        let n = d.index_count as usize;
        let idx = &mesh.indices[first..first + n];
        let min = *idx.iter().min().unwrap() as usize;
        let rel = |i: u16| i as usize - min;
        let at = |v: usize, o: usize| mesh.layer_bytes.get(layer as usize + v * stride + o).copied();
        let mut seed = 12345u32;
        for t in idx.chunks(3) {
            for k in 0..3 {
                let (a, b) = (rel(t[k]), rel(t[(k + 1) % 3]));
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let c = rel(idx[(seed >> 8) as usize % n]);
                for o in 0..stride {
                    if let (Some(x), Some(y), Some(z)) = (at(a, o), at(b, o), at(c, o)) {
                        let s = &mut e.1[o];
                        s.0 += (x as f64 - y as f64).abs();
                        s.1 += (x as f64 - z as f64).abs();
                        s.2 += 1;
                    }
                }
            }
        }
    }
    let mut kinds: HashMap<u8, ()> = HashMap::new();
    for (f, (stride, per, draws, ts)) in &acc {
        kinds.insert(*f, ());
        println!("format {f}: stride {stride}, {draws} draws, techsets {ts:?}");
        if let Some(l) = layouts.get(f) {
            println!("  declared {:?} stride {}", l.components, l.stride);
        }
        let row: Vec<String> = per
            .iter()
            .enumerate()
            .map(|(o, s)| format!("{o}:{:.2}", if s.1 > 0.0 { s.0 / s.1 } else { f64::NAN }))
            .collect();
        println!("  coherence (neighbour diff / random diff; low = smooth field) {}", row.join(" "));
    }
}
