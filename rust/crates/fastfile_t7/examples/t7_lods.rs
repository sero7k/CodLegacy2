//! `t7_lods <zone.ff> [count]`: models with more than one LOD, the header
//! words between the LOD count (+0x40) and the LOD meshes (+0x88) as
//! floats, to read the LOD distances.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let n: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    for m in w.capture.models.iter().filter(|m| m.lods.len() > 1).take(n) {
        let h = m.header_at;
        let f: Vec<String> = (0x40..0x88).step_by(4).map(|o| {
            let v = u32::from_le_bytes(z.image[h + o..h + o + 4].try_into().unwrap());
            format!("{o:#x}:{:.6}/{v:08x}", f32::from_bits(v))
        }).collect();
        println!("{} lods {}: {}", m.name, m.lods.len(), f.join(" "));
    }
}
