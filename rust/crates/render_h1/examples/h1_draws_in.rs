//! The scene draws whose bounds' centre lies in a box, by material:
//! count, mesh (0 world, 1 static models), techset and triangles.
//!
//! `cargo run -p render_h1 --release --example h1_draws_in -- <x0> <y0> <x1> <y1> <map.ff> <zone.ff>...`
use std::collections::BTreeMap;

use render_h1::scene::Scene;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let b: Vec<f32> = a[..4].iter().map(|s| s.parse().expect("box")).collect();
    let zones: Vec<std::path::PathBuf> = a[4..].iter().map(Into::into).collect();
    let scene = Scene::load(&zones).expect("load");
    let bounds = render_h1::world::draw_bounds(&scene);
    let layered: Vec<u32> = scene.draws.iter().filter_map(|d| d.layer).collect();
    let mut by_mod = [0usize; 4];
    for l in &layered {
        by_mod[(l % 4) as usize] += 1;
    }
    let world_stride = scene.meshes[0].kind.stride();
    let main_mis = scene
        .draws
        .iter()
        .filter(|d| d.layer.is_some() && (u64::from(d.base_vertex as u32) * world_stride) % 4 != 0)
        .count();
    println!(
        "layered draws {}, layer offset mod 4 {by_mod:?}, world stride {world_stride}, main offset misaligned {main_mis}, layer bytes {}",
        layered.len(),
        scene.meshes[0].layer_bytes.len()
    );
    let mut by: BTreeMap<(usize, String), (usize, u32, f32, f32)> = BTreeMap::new();
    for (d, bb) in scene.draws.iter().zip(&bounds) {
        let Some((lo, hi)) = bb else { continue };
        let c = (*lo + *hi) * 0.5;
        // H1_BIG=1: draws that cover the whole box instead.
        if std::env::var("H1_BIG").is_ok() {
            if lo.x > b[0] || lo.y > b[1] || hi.x < b[2] || hi.y < b[3] {
                continue;
            }
        } else if c.x < b[0] || c.x > b[2] || c.y < b[1] || c.y > b[3] {
            continue;
        }
        let m = &scene.materials[d.material];
        if let Ok(w) = std::env::var("H1_DRAW_LIST")
            && m.name == w
        {
            println!("  draw {}: {lo:?}..{hi:?} layer {:?} base {} first {} count {}", m.name, d.layer, d.base_vertex, d.first_index, d.index_count);
        }
        let e = by
            .entry((d.mesh, format!("{} [{}]", m.name, m.techset)))
            .or_insert((0, 0, f32::MAX, f32::MIN));
        e.0 += 1;
        e.1 += d.index_count / 3;
        e.2 = e.2.min(lo.z);
        e.3 = e.3.max(hi.z);
    }
    for ((mesh, name), (n, tris, z0, z1)) in &by {
        println!("mesh {mesh} {name}: {n} draws, {tris} tris, z {z0:.0}..{z1:.0}");
    }
}
