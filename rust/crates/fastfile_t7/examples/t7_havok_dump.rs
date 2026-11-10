//! `t7_havok_dump <zone.ff> <out dir>`: each Havok packfile in the zone's
//! image (magic `57 E0 E0 57 10 C0 C0 10`; the nav mesh and nav volume
//! assets hold them) written out as `<offset>.hkx`, cut at the next one
//! (or 64 MB), with its version and section names printed.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let img = &z.image;
    let magic = [0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10];
    let mut starts = Vec::new();
    let mut i = 0;
    while let Some(p) = img[i..].windows(8).position(|w| w == magic) {
        starts.push(i + p);
        i += p + 8;
    }
    for (k, &s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(img.len()).min(s + (64 << 20));
        let h = &img[s..end];
        let version: String = h[0x28..0x38].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
        let sections = i32::from_le_bytes([h[0x14], h[0x15], h[0x16], h[0x17]]);
        println!("{s:#x}: {} bytes, file version {}, {version}, {sections} sections, layout {:?}", end - s, i32::from_le_bytes([h[0xc], h[0xd], h[0xe], h[0xf]]), &h[0x10..0x14]);
        if let Some(dir) = a.get(2) {
            std::fs::write(std::path::Path::new(dir).join(format!("{s:x}.hkx")), h).expect("write");
        }
    }
}
