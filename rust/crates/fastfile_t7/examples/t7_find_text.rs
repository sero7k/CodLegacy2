//! `t7_find_text <zone.ff> <needle>`: each place the needle occurs in the
//! zone's image, with the text around it (rawfile contents the walk does
//! not keep, such as vision files).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let n = a[2].as_bytes();
    let img = &z.image;
    let mut i = 0;
    while let Some(p) = img[i..].windows(n.len()).position(|w| w == n) {
        let at = i + p;
        let lo = at.saturating_sub(160);
        let hi = (at + 1600).min(img.len());
        let text: String = img[lo..hi].iter().map(|&b| if b == b'\n' || (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
        println!("== {at:#x}\n{text}\n");
        i = at + n.len();
    }
}
