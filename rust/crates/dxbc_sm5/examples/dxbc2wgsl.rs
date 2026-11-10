//! `cargo run -p dxbc_sm5 --example dxbc2wgsl -- <file.dxbc>`: prints the WGSL and reflection.
fn main() {
    let p = std::env::args().nth(1).expect("dxbc file");
    let b = std::fs::read(&p).expect("read");
    let c = dxbc_sm5::Container::parse(&b).expect("container");
    let prog = dxbc_sm5::decode(c.shader).expect("decode");
    let t = dxbc_sm5::translate_program(&prog, &c, &dxbc_sm5::Options::for_stage(prog.stage))
        .expect("translate");
    println!("{}", t.wgsl);
    eprintln!("{:#?}", t.reflection);
}
