//! bo2mp: `t7_sky_check <map.ff>`: the sky draw the game's scene makes
//! (`Scene::load`, the path the game takes): its material, whether its
//! forward (sky) technique was made, and why not. Exits 1 when the map has
//! a sky model but no sky draw with a technique (the PC's `0 sky box draws
//! issued per frame` on Shadows of Evil); offline without the base
//! packages, when no sky material has its technique.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    println!("sky models: {:?}", scene.map.sky_models);
    for l in scene.report.iter().filter(|l| l.contains("sky") || l.contains("zones opened")) {
        println!("scene: {l}");
    }
    let mut ready = 0;
    for d in scene.draws.iter().filter(|d| d.list == 7 && scene.materials[d.material].techset.contains("sky")) {
        let m = &scene.materials[d.material];
        println!(
            "sky draw: {} ({}): forward technique {}, undrawn {:?}",
            m.name,
            m.techset,
            if m.forward.is_some() { "yes" } else { "no" },
            m.undrawn
        );
        ready += usize::from(m.forward.is_some());
    }
    println!("sky draws ready: {ready}");
    let undrawn = scene.materials.iter().filter(|m| m.undrawn.is_some()).count();
    println!("materials: {} ({undrawn} not drawn)", scene.materials.len());
    // (Offline the sky model's mesh streams from the base packages, which
    // may not be here: then its material, which the PC lacked, is checked.)
    let mut mats = 0;
    for m in scene.materials.iter().filter(|m| m.techset.contains("sky_latlong")) {
        println!("sky material: {} ({}): forward technique {}, undrawn {:?}", m.name, m.techset, if m.forward.is_some() { "yes" } else { "no" }, m.undrawn);
        mats += usize::from(m.forward.is_some());
    }
    // (A Workshop map's sky materials may use another sky technique set
    // than `sky_latlong`: every material whose set or name says sky.)
    for m in scene.materials.iter().filter(|m| !m.techset.contains("sky_latlong") && (m.techset.contains("sky") || m.name.contains("sky"))) {
        println!("other sky material: {} ({}): forward technique {}, undrawn {:?}", m.name, m.techset, if m.forward.is_some() { "yes" } else { "no" }, m.undrawn);
        mats += usize::from(m.techset.contains("sky") && m.forward.is_some());
    }
    for m in scene.materials.iter().filter(|m| m.undrawn.is_some()) {
        println!("not drawn: {} ({}): {:?}", m.name, m.techset, m.undrawn);
    }
    if !scene.map.sky_models.is_empty() && ready == 0 && mats == 0 {
        std::process::exit(1);
    }
}
