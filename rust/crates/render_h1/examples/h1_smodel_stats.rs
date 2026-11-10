//! Static model lighting fields: primary light weight, ground lighting.
fn main() {
    let p = std::env::args().nth(1).expect("zone");
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    let map = fastfile_h1::map::read_map(&cap, &z.image, &z.assets.script_strings);
    let w = map.world.expect("world");
    let mut hist = [0usize; 11];
    for sm in &w.static_models {
        if let Some(v) = sm.primary_light_weight {
            hist[((v.clamp(0.0, 1.0)) * 10.0) as usize] += 1;
        }
    }
    println!("weight histogram (0..1 in tenths): {hist:?}");
    for sm in w.static_models.iter().take(12) {
        let name = sm
            .model
            .map(|m| cap.name_of(m).to_string())
            .unwrap_or_default();
        println!(
            "{name} flags {:#x} weight {:?} ground {:?} origin {:?}",
            sm.flags, sm.primary_light_weight, sm.ground_lighting, sm.origin
        );
    }
}
