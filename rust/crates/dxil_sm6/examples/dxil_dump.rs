//! Prints a DXIL program as text: `dxil_dump <file.dxbc>` where the file is
//! one `DXBC` container (see `fastfile_jup`'s `jup_shaders --dump`).
fn main() {
    let path = std::env::args().nth(1).expect("usage: dxil_dump <file.dxbc>");
    let b = std::fs::read(path).expect("read");
    let n = u32::from_le_bytes(b[28..32].try_into().unwrap()) as usize;
    for k in 0..n {
        let o = u32::from_le_bytes(b[32 + 4 * k..36 + 4 * k].try_into().unwrap()) as usize;
        if &b[o..o + 4] == b"DXIL" {
            let len = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap()) as usize;
            match dxil_sm6::read_part(&b[o + 8..o + 8 + len]) {
                Ok(m) => print!("{}", dxil_sm6::text::dump(&m)),
                Err(e) => eprintln!("error: {e}"),
            }
        }
    }
}
