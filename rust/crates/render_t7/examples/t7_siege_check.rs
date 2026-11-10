//! `t7_siege_check <map.ff>`: the map's auto-played siege animations as the
//! scene starts them (`render_t7::siege`), sampled headless: per playing
//! static model its animation and how far its parts' bones move over the
//! first two seconds (frame clock), against the rest pose.
fn main() {
    let zone = std::path::PathBuf::from(std::env::args().nth(1).expect("map zone"));
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    for l in scene.report.iter().filter(|l| l.contains("siege")) {
        println!("scene: {l}");
    }
    let mut s = scene.siege.lock().expect("siege").clone();
    println!("playing: {}", s.playing.len());
    // Each playing model's bones at 0 s, then every 0.1 s to 2 s.
    let mut first: std::collections::HashMap<u32, Vec<[f32; 16]>> = Default::default();
    s.update(0.0, &mut |b, m| {
        first.insert(b, m.to_vec());
    });
    let mut far: std::collections::HashMap<u32, f32> = Default::default();
    for k in 1..=20 {
        s.update(k as f32 * 0.1, &mut |b, m| {
            let f = &first[&b];
            let d = m.iter().zip(f).map(|(x, y)| (0..12).map(|i| (x[i] - y[i]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
            let e = far.entry(b).or_default();
            *e = e.max(d);
        });
    }
    let mut moved = 0;
    for (i, p) in s.playing.iter().enumerate() {
        let d = far.get(&p.first_bone).copied().unwrap_or(0.0);
        if d > 1e-3 {
            moved += 1;
        }
        if i < 12 {
            println!("model {i}: {} ({} parts) at frame {:.2}: bone values change by up to {d:.3} over 2 s", s.anims[p.anim].header.name, p.parts, p.track.frame);
        }
    }
    println!("{moved} of {} playing models move", s.playing.len());
}
