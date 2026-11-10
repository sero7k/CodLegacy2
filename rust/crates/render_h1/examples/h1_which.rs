//! Which technique each material matching a pattern draws with, per lit
//! slot, and its pixel shader.
//!
//! `cargo run -p render_h1 --example h1_which -- <pattern> <map.ff> <zone.ff>...`

use render_h1::scene::Scene;
use render_h1::world::{pick, slot};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let zones: Vec<std::path::PathBuf> = a[1..].iter().map(Into::into).collect();
    let scene = Scene::load(&zones).expect("load");
    for m in scene
        .materials
        .iter()
        .filter(|m| m.name.contains(a[0].as_str()))
    {
        println!("{} [{}]", m.name, m.techset);
        if std::env::var("H1_WHICH_TEX").is_ok() {
            for t in &m.textures {
                println!(
                    "  tex hash {:08x} sampler {} semantic {} image {}",
                    t.0, t.1, t.2, t.3
                );
            }
        }
        for s in [
            slot::LIT_DIR_SHADOW,
            slot::LIT_DIR,
            slot::LIT,
            slot::ZPREPASS,
        ] {
            if let Some((sl, t)) = pick(&scene.pool, &m.techset, &[s]) {
                let p = t.pass_info.first();
                println!(
                    "  slot {sl}: {} ps {:?}",
                    t.name,
                    p.and_then(|p| p.pixel_shader.clone())
                );
            }
        }
    }
}
