//! One scene draw's first vertices: position, the clip position the view
//! gives them, and the raw layer stream bytes (debug).
//!
//! `cargo run -p render_h1 --release --example h1_draw_probe -- <first_index> <x y z pitch yaw> <map.ff> <zone.ff>...`
use glam::Vec3;
use render_h1::code::{Camera, View};
use render_h1::scene::Scene;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let first: u32 = a[0].parse().expect("first index");
    let c: Vec<f32> = a[1..6].iter().map(|s| s.parse().expect("cam")).collect();
    let zones: Vec<std::path::PathBuf> = a[6..].iter().map(Into::into).collect();
    let scene = Scene::load(&zones).expect("load");
    let view = View {
        eye: Vec3::new(c[0], c[1], c[2]),
        angles: Vec3::new(c[3], c[4], 0.0),
        fov_x: 80.0,
        width: 320,
        height: 180,
        near: 4.0,
    };
    let cam = Camera::from(&view);
    let Some(d) = scene.draws.iter().find(|d| d.first_index == first) else {
        println!("no draw at {first}");
        return;
    };
    let m = &scene.meshes[d.mesh];
    let stride = m.kind.stride() as usize;
    println!(
        "draw mesh {} base {} first {} count {} layer {:?} material {}",
        d.mesh,
        d.base_vertex,
        d.first_index,
        d.index_count,
        d.layer,
        scene.materials[d.material].name
    );
    let idx = &m.indices[d.first_index as usize..(d.first_index + d.index_count) as usize];
    let max_i = idx.iter().copied().max().unwrap_or(0);
    println!("index max {max_i}, min {}", idx.iter().copied().min().unwrap_or(0));
    let degenerate = idx
        .chunks_exact(3)
        .filter(|t| t[0] == t[1] || t[1] == t[2] || t[0] == t[2])
        .count();
    println!(
        "triangles {}, degenerate {degenerate}, first indices {:?}",
        idx.len() / 3,
        &idx[..idx.len().min(12)]
    );
    for &i in idx.iter().take(4) {
        let v = (d.base_vertex + i32::from(i)) as usize * stride;
        let b = &m.vertex_bytes[v..v + stride];
        let f = |k: usize| f32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]]);
        let p = Vec3::new(f(0), f(1), f(2));
        let clip = cam.view_projection * (p - cam.eye).extend(1.0);
        let ndc = clip / clip.w;
        let lb = d.layer.map(|l| {
            let o = l as usize + i as usize * 24;
            m.layer_bytes.get(o..o + 24).map(|s| s.to_vec())
        });
        println!("  v{i}: pos {p} w {:.3} clip {clip} ndc {ndc}\n     layer {lb:?}", f(3));
    }
}
