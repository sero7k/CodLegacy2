//! `t7_lights <map.ff>`: the map's primary lights (`ComPrimaryLight`, 564
//! bytes): type word (+0x40), origin (+0x48), the light def name (+400)
//! and the first words, to read the layout off.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let im = &z.image;
    let u = |o: usize| u32::from_le_bytes(im[o..o + 4].try_into().unwrap());
    let f = |o: usize| f32::from_bits(u(o));
    for c in &w.capture.world.com_worlds {
        let Some((at, n)) = c.lights else { continue };
        println!("{}: {n} primary lights", c.name);
        let mut types = std::collections::BTreeMap::new();
        for i in 0..n {
            let r = at + 564 * i;
            *types.entry(u(r + 0x40)).or_insert(0) += 1;
            if i < 12 || std::env::var("ALL").is_ok() {
                let name_ptr = &im[r + 400..r + 408];
                let words: Vec<String> = (0..24).map(|k| format!("{:08x}", u(r + 0x40 + 4 * k))).collect();
                println!("  {i}: type {} origin ({:.0} {:.0} {:.0}) +0x40.. {} name {:02x?}", u(r + 0x40), f(r + 0x48), f(r + 0x4c), f(r + 0x50), words.join(" "), name_ptr);
            }
        }
        println!("types: {types:?}");
        for i in 0..3 {
            let r = at + 564 * i;
            println!("  light {i}: colour ({:.2} {:.2} {:.2}) 0x64 {:.2} atten 0x68..0x74 ({:.1} {:.1} {:.2} {:.2}) 0x78 {} fade 0x7c {:.2} 0x80 {:.2} shadow 0x84 {} 0x94 {:.2} dir ({:.2} {:.2} {:.2}) 0x110 {:.3} 0x114 {:.3} 0x118.. ({:.2} {:.2} {:.2} {:.2}) 0x12c {:.1} radius 0x14c {:.1} mask 0x158 {:#x} 0x15c {} 0x160 {:.2} bytes 0x164 {} {} bounds ({:.0} {:.0} {:.0})..({:.0} {:.0} {:.0})",
                f(r+0x58), f(r+0x5c), f(r+0x60), f(r+0x64), f(r+0x68), f(r+0x6c), f(r+0x70), f(r+0x74), im[r+0x78], f(r+0x7c), f(r+0x80), u(r+0x84), f(r+0x94), f(r+0x9c), f(r+0xa0), f(r+0xa4), f(r+0x110), f(r+0x114), f(r+0x118), f(r+0x11c), f(r+0x120), f(r+0x124), f(r+0x12c), f(r+0x14c), u(r+0x158), im[r+0x15c], f(r+0x160), im[r+0x164], im[r+0x165], f(r+0x1f0), f(r+0x1f4), f(r+0x1f8), f(r+0x1fc), f(r+0x200), f(r+0x204));
        }
    }
}
