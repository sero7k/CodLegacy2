//! `t7_gfxworld_dump <map.ff> <from> <to>`: the GfxWorld header's words in
//! a byte range (hex), as floats and hex, for finding settings blocks.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let g = w.capture.world.gfx_worlds.first().expect("gfxworld");
    let p = |s: &str| usize::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
    let (from, to) = (p(&a[2]), p(&a[3]));
    // T7_DUMP_SUN=1: the first sun volume instead of the header.
    let base = if std::env::var("T7_DUMP_SUN").is_ok() { g.sun_volumes[0] } else { g.header_at };
    println!("header at {:#x} base {base:#x}", g.header_at);
    for o in (from..to).step_by(16) {
        let ws: Vec<String> = (0..4).map(|k| {
            let at = base + o + 4 * k;
            let u = u32::from_le_bytes(z.image[at..at + 4].try_into().unwrap());
            let f = f32::from_bits(u);
            if f.is_finite() && (f == 0.0 || (f.abs() > 1e-6 && f.abs() < 1e7)) { format!("{f:>12.5}") } else { format!("  0x{u:08x}") }
        }).collect();
        println!("{o:#06x} {}", ws.join(" "));
    }
}
