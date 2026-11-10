//! `iw7_post_programs <code_post_gfx.ff> <name part | 0xOFFSET,... | LIST> <out dir>`: finds the
//! zone's shader programs by scanning for their DXBC containers (the asset walk stops at the
//! zone's DDL assets), names each by the source name stored before it, and writes the WGSL
//! translation of those matching (or lists them all).
use dxbc_sm5::{Container, Options, decode, translate_program};
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let want = std::env::args().nth(2).unwrap();
    let out = std::env::args().nth(3).unwrap();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&p)).unwrap();
    let img = &zone.image;
    let mut i = 0;
    let mut k = 0;
    while let Some(off) = img[i..].windows(4).position(|w| w == b"DXBC") {
        let at = i + off;
        i = at + 4;
        let size = u32::from_le_bytes(img[at + 24..at + 28].try_into().unwrap()) as usize;
        let back = &img[at.saturating_sub(200)..at];
        let txt: String = back.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '|' }).collect();
        let name: String = txt.split('|').filter(|s| s.len() > 6).last().unwrap_or("").to_owned();
        if at + size > img.len() { continue; }
        let Ok(c) = Container::parse(&img[at..at + size]) else { continue };
        let Ok(pr) = decode(c.shader) else { println!("{name}: decode failed"); continue };
        if want == "LIST" { println!("{at:#x} {:?} {name}", pr.stage); continue; }
        let by_off = want.split(',').any(|w| w.strip_prefix("0x").and_then(|h| usize::from_str_radix(h, 16).ok()) == Some(at));
        if !by_off && !name.contains(&want) { continue; }
        match translate_program(&pr, &c, &Options::for_stage(pr.stage)) {
            Ok(t) => {
                let f = format!("{out}/{k:03}_{:?}_{}.wgsl", pr.stage, name.replace('/', "_"));
                std::fs::write(&f, &t.wgsl).unwrap();
                println!("{f}");
            }
            Err(e) => println!("{name}: {e:?}"),
        }
        k += 1;
    }
}
