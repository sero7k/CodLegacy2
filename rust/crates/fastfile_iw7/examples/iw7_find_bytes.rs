//! `iw7_find_bytes <zone.ff> <hex>`: every offset of a byte pattern in the decompressed image.
fn main() {
    let mut a = std::env::args().skip(1);
    let zone =
        fastfile_iw7::Zone::open(std::path::Path::new(&a.next().expect("zone"))).expect("open");
    let pat: Vec<u8> = a
        .next()
        .expect("hex")
        .as_bytes()
        .chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect();
    for (i, w) in zone.image.windows(pat.len()).enumerate() {
        if w == pat.as_slice() {
            println!("{i:#x}");
        }
    }
}
