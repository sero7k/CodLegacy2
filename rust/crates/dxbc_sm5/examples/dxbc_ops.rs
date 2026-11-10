//! `dxbc_ops <file.dxbc>...`: each program's stage and how often each opcode
//! appears (to see what a translation needs).
fn main() {
    let mut all = std::collections::BTreeMap::new();
    for p in std::env::args().skip(1) {
        let b = std::fs::read(&p).expect("read");
        let Ok(c) = dxbc_sm5::Container::parse(&b) else { continue };
        let Ok(prog) = dxbc_sm5::decode(c.shader) else { println!("{p}: decode failed"); continue };
        for i in &prog.instrs {
            *all.entry(i.op).or_insert(0usize) += 1;
        }
        println!("{p}: {:?} {} instrs", prog.stage, prog.instrs.len());
    }
    println!("{all:?}");
}
