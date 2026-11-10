//! `t7_gsc_scan <zone.ff> [out_dir]`: every compiled GSC/CSC object in the
//! zone's image (found by its `\x80GSC\r\n\0` magic): offset, version byte,
//! and the header words; with `out_dir`, each object written out.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let img = &z.image;
    let magic = b"\x80GSC\r\n\0";
    let mut i = 0;
    let mut n = 0;
    while let Some(p) = img[i..].windows(7).position(|w| w == magic) {
        let at = i + p;
        let h = &img[at..(at + 0x60).min(img.len())];
        let words: Vec<u32> = h[8..].chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], *c.get(2).unwrap_or(&0), *c.get(3).unwrap_or(&0)])).collect();
        let name_off = words[11] as usize;
        let name: String = img[at + name_off..].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        println!("{at:#x} v{:#x} {name} cseg={:#x}+{:#x} exports={} imports={} strings={} includes={}", h[7], words[3], words[10], words[12] >> 16, words[13] & 0xffff, words[12] & 0xffff, h[0x44.min(h.len() - 1)]);
        if let Some(dir) = a.get(2) {
            // (The object ends at the last of its tables; writes a generous
            // window, cut at the next object.)
            let next = img[at + 7..].windows(7).position(|w| w == magic).map_or(img.len(), |q| at + 7 + q);
            let end = next.min(at + 0x400000);
            let path = std::path::Path::new(dir).join(name.replace('/', "__"));
            std::fs::write(path, &img[at..end]).expect("write");
        }
        n += 1;
        i = at + 7;
    }
    println!("{n} objects");
}
