//! Origins of the placed static models whose model name holds a word:
//! `cargo run -p fastfile_h1 --release --example h1_model_at -- <zone.ff> <word>`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&args[0])).expect("zone");
    let (map, cap) = fastfile_h1::map::IwMap::load(&zone);
    let Some(world) = map.world.as_ref() else { return };
    for s in &world.static_models {
        let Some(m) = s.model else { continue };
        let name = cap.name_of(m);
        if name.contains(args[1].as_str()) {
            println!("{name} at {:.0} {:.0} {:.0}", s.origin[0], s.origin[1], s.origin[2]);
        }
    }
}
