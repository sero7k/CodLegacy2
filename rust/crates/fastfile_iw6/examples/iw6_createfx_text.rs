//! Prints a map zone's createfx raw text (head) and what `createfx::read_text` reads from it:
//! `iw6_createfx_text <mp_map.ff> [--raw]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&a[0])).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let name = a[0].rsplit('/').next().unwrap().trim_end_matches(".ff").to_owned();
    let get = |p: String| fastfile_iw6::map::raw_file(&cap, &zone.image, &p).map(|b| String::from_utf8_lossy(&b).into_owned());
    let cfx = get(format!("maps/createfx/{name}_fx.gsc"));
    let fx = get(format!("maps/mp/{name}_fx.gsc"));
    eprintln!("createfx text {:?} bytes, fx text {:?}", cfx.as_ref().map(String::len), fx.as_ref().map(String::len));
    if a.iter().any(|x| x == "--raw") {
        println!("{}", cfx.as_deref().unwrap_or("(none)"));
        println!("=======\n{}", fx.as_deref().unwrap_or("(none)"));
    }
}
