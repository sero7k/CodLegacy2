//! A zone's materials' raw constant buffers: `t7_matraw <zone.ff>
//! <substring>` prints each matching material's technique set and its
//! globals as floats.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    for m in w.capture.materials.iter().filter(|m| m.name.contains(&a[2])) {
        println!("{} {:?}", m.name, m.techset);
        for (h, t) in &m.textures {
            println!("    {h:#010x} {t}");
        }
        for (slot, g) in &m.globals {
            let fl: Vec<String> = g.chunks(4).map(|c| format!("{:.4}", f32::from_le_bytes(c.try_into().unwrap()))).collect();
            println!("    globals[{slot}] {}", fl.join(" "));
        }
    }
}
