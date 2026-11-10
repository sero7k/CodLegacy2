//! Lists a zone's vision files (`vision/*.vision` rawfiles) and parses
//! each with the exe's table: `t7_vision <zone.ff> [name substring]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let w = fastfile_t7::walk::walk(&zone);
    println!("walk: {:?}", w.stop);
    for x in &w.walked {
        let Some(name) = &x.name else { continue };
        if x.ty != fastfile_t7::AssetType::RawFile || !name.ends_with(".vision") || !name.contains(&want) {
            continue;
        }
        let body = &zone.image[x.start..x.end];
        // (RawFile: 24-byte header, the name, then the text.)
        let name_end = body.windows(name.len()).position(|w| w == name.as_bytes()).map_or(24, |p| p + name.len() + 1);
        let text: String = String::from_utf8_lossy(&body[name_end.min(body.len())..]).chars().filter(|c| *c != '\0').collect();
        let v = fastfile_t7::native::parse_vision(&text);
        if std::env::var_os("T7_VISION_TEXT").is_some() { println!("{text}"); }
        println!("{name}: {:?} lut {:?} unknown {:?}", v.words, v.film_lut, v.unknown);
    }
}
