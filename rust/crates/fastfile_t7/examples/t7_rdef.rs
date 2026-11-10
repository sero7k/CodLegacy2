//! Prints each `.dxbc` file's constant buffers (with their variables) and
//! texture slots: `t7_rdef <file.dxbc>...` (files as `t7_dxbc` writes them).
fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read");
        let Some(r) = fastfile_t7::dxbc::reflect(&bytes) else {
            println!("{path}: no RDEF");
            continue;
        };
        println!("== {path}");
        for b in &r.buffers {
            println!("  cbuffer {} ({} bytes)", b.name, b.size);
            for v in &b.variables {
                println!("    {:#06x} {:4} {}{}", v.offset, v.size, v.name, if v.used { "" } else { " (unused)" });
            }
        }
        for (t, slot) in &r.textures {
            println!("  texture t{slot} {t}");
        }
    }
}
