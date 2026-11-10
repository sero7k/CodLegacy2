//! Writes a Horizon MW killstreak picture as a PAM file:
//! `cargo run -p fastfile_h1 --release --example h1_ks_icon -- <hmw zone dir> <material> <out.pam>`.

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::Path::new(&a[0]);
    let (w, h, px) = fastfile_h1::ui_image::read_hmw_killstreak_icon(dir, &[a[1].clone()]).expect("no such picture");
    println!("{w}x{h}");
    let mut out = format!("P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n").into_bytes();
    out.extend(px);
    std::fs::write(&a[2], out).unwrap();
}
