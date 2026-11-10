//! Loads a Ghosts map and prints its `iw6 sky:` / sun / vision / fog notes and
//! the sky draws and the stand-in's pixels.
//!
//! `cargo run -p render_iw6 --example iw6_sky_notes -- <map.ff>`
use render_iw6::scene::{List, Scene};

fn main() {
    let p: std::path::PathBuf = std::env::args().nth(1).expect("map.ff").into();
    let zones = Scene::zones_for(&p);
    let mut scene = Scene::load(&zones).expect("load");
    for n in scene.notes.iter().filter(|n| n.contains("sky") || n.starts_with("iw6 sun") || n.starts_with("iw6 vision") || n.starts_with("iw6 fog")) {
        println!("{n}");
    }
    for d in scene.draws.iter().filter(|d| d.list == List::Sky) {
        let m = &scene.materials[d.material];
        println!("sky draw: material {} techset {} indices {}", m.name, m.techset, d.index_count);
        for t in &m.textures {
            println!("   texture {:?}", t);
        }
    }
    let cube = scene.images.synthetic.keys().cloned().collect::<Vec<_>>();
    println!("synthetic: {cube:?}");
    for k in cube {
        if let Some((px, faces)) = scene.images.image(&k) {
            println!("  {k}: {}x{} faces {faces} dxgi {}", px.width, px.height, px.dxgi_format);
        }
    }
}
