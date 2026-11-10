//! Lists the shader programs of a decompressed zone image (`code_post_gfx`):
//! `jup_shaders <file.zone> [--dump <index> <out.dxbc>]`. Per program: stage,
//! size, hlsl name when present, bindings and signature semantics; then a
//! summary by stage.
use fastfile_jup::shader::{self, Stage};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = std::fs::read(a.get(1).expect("usage: jup_shaders <file.zone>")).expect("read");
    let progs = shader::scan(&z);
    if a.get(2).map(String::as_str) == Some("--dump") {
        let i: usize = a[3].parse().expect("index");
        let p = &progs[i];
        std::fs::write(&a[4], &z[p.at..p.at + p.size]).expect("write");
        return;
    }
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for (i, p) in progs.iter().enumerate() {
        *by.entry(format!("{:?}", p.stage)).or_default() += 1;
        if matches!(p.stage, Stage::Pixel | Stage::Vertex) || a.iter().any(|x| x == "--all") {
            let b: Vec<String> = p
                .bindings
                .iter()
                .map(|b| format!("{:?}{}:{}-{}k{}", b.class, b.space, b.lower, b.upper, b.kind))
                .collect();
            let o: Vec<String> = p
                .outputs
                .iter()
                .map(|e| format!("{}{}", e.semantic, e.index))
                .collect();
            println!(
                "{i} {:#x} {:?} {} {} out[{}] bind[{}]",
                p.at,
                p.stage,
                p.size,
                p.name,
                o.join(","),
                b.join(" ")
            );
        }
    }
    println!("{} programs: {by:?}", progs.len());
}
