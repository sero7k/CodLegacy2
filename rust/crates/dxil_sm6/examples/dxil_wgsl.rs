//! Lowers a `DXBC` container's DXIL program to WGSL and validates it with
//! naga: `dxil_wgsl <file.dxbc> [out.wgsl]`. Prints the binding table.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = std::fs::read(a.get(1).expect("usage: dxil_wgsl <file.dxbc> [out.wgsl]")).expect("read");
    let m = read_dxil(&b).expect("no DXIL part");
    match dxil_sm6::wgsl::translate(&m) {
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
        Ok(t) => {
            if let Some(o) = a.get(2) {
                std::fs::write(o, &t.wgsl).expect("write");
            }
            for bd in &t.bindings {
                println!(
                    "group {} binding {} {:?} {} kind {} size {}",
                    bd.group, bd.binding, bd.range.class, bd.name, bd.range.kind, bd.range.size
                );
            }
            match validate(&t.wgsl) {
                Ok(()) => println!("naga: valid ({} bytes of WGSL)", t.wgsl.len()),
                Err(e) => {
                    eprintln!("naga: {e}");
                    std::process::exit(2);
                }
            }
        }
    }
}

fn read_dxil(b: &[u8]) -> Option<dxil_sm6::Module> {
    let n = u32::from_le_bytes(b[28..32].try_into().ok()?) as usize;
    for k in 0..n {
        let o = u32::from_le_bytes(b[32 + 4 * k..36 + 4 * k].try_into().ok()?) as usize;
        if &b[o..o + 4] == b"DXIL" {
            let len = u32::from_le_bytes(b[o + 4..o + 8].try_into().ok()?) as usize;
            return dxil_sm6::read_part(&b[o + 8..o + 8 + len]).ok();
        }
    }
    None
}

fn validate(src: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| e.emit_to_string(src))?;
    let caps = naga::valid::Capabilities::all();
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&module)
        .map_err(|e| e.emit_to_string(src))?;
    Ok(())
}
