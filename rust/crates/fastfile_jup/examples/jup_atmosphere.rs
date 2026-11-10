//! Lists a Modern Warfare III map's vision set (fog and the sun-fog
//! direction, then every other setting by key hash) and its placed lights:
//! `jup_atmosphere <map zone .ff>`

fn main() {
    let Some(zone) = std::env::args().nth(1) else {
        eprintln!("usage: jup_atmosphere <map zone .ff>");
        std::process::exit(2);
    };
    let src = fastfile_jup::map::MapSources::beside(std::path::Path::new(&zone));
    let oodle = fastfile_jup::oodle::load(&src.install).expect("oodle");
    let image = fastfile_jup::map::read_image(&src.zone(&src.name), oodle.as_ref()).expect("zone");
    let sets = fastfile_jup::map::atmosphere::scan(&image.bytes);
    println!("{} vision sets", sets.len());
    for (k, s) in sets.iter().enumerate() {
        println!("set {k}: name {:?}, {} settings", s.name(), s.values.len());
    }
    if let Some(a) = fastfile_jup::map::atmosphere::Atmosphere::read(&image.bytes) {
        println!("{}: {:#?}", a.name, a.fog);
        println!("toward the sun: {:?}", a.toward_sun());
        for (k, v) in &a.set.values {
            println!("  {k:016x} {v}");
        }
    }
}
