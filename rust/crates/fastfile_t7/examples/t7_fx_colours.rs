//! Each element's visual samples' colours (RGBA bytes, word 6 the emissive
//! stops): `t7_fx_colours <zone.ff> <effect substring>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    for fx in w.capture.fx.iter().filter(|f| f.name.contains(&a[2])) {
        println!("{}", fx.name);
        for (i, e) in fx.elems.iter().enumerate() {
            let s = &e.samples[2];
            let cols: Vec<String> = s
                .chunks(40)
                .take(4)
                .map(|c| format!("({},{},{},{}) w{:.2}", c[0], c[1], c[2], c[3], f32::from_le_bytes(c[24..28].try_into().unwrap())))
                .collect();
            println!("  [{i}] type {} {:?} {}", e.raw[0xc8], e.visuals, cols.join(" "));
        }
    }
}
