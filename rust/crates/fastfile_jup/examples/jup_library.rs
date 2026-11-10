//! One line per graphics program of a decoded `code_post_gfx` zone: index,
//! stage, record key, outputs, sample count and the constant offsets read
//! from each raw buffer. `jup_library <code_post_gfx.zone> [--all]`
//! (compute programs too with `--all`). Tab separated.
use fastfile_jup::shader::{self, Stage};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = std::fs::read(&a[1]).expect("read");
    let all = a.iter().any(|x| x == "--all");
    let progs = shader::scan(&z);
    for (i, p) in progs.iter().enumerate() {
        if !all && !matches!(p.stage, Stage::Pixel | Stage::Vertex) {
            continue;
        }
        let outs: Vec<String> = p.outputs.iter().map(|o| format!("{}{}", o.semantic, o.index)).collect();
        let Ok(m) = dxil_sm6::read_part(&z[p.bitcode.clone()]) else {
            println!("{i}\t{:?}\tunreadable", p.stage);
            continue;
        };
        let Some(e) = dxil_sm6::meta::entry(&m) else {
            println!("{i}\t{:?}\tno entry", p.stage);
            continue;
        };
        let prof = dxil_sm6::analysis::profile(&m, &e);
        let mut reads = Vec::new();
        for (key, offs) in &prof.buffer_reads {
            let r = e.ranges.iter().find(|r| r.class == key.0 && r.space == key.1 && r.lower == key.2);
            if r.is_some_and(|r| matches!(r.kind, 11 | 12 | 10)) {
                let o: Vec<String> = offs.keys().map(|k| k.to_string()).collect();
                reads.push(format!("{:?}{}:{}[{}]", key.0, key.1, key.2, o.join(",")));
            }
        }
        println!(
            "{i}\t{:?}\t{}\t{}\t{:#x}\t{}\t{}\t{}\t{}\t{}",
            p.stage,
            p.key.family,
            p.key.feature_mask,
            p.key.pass_mask,
            outs.join(","),
            p.is_bindless(),
            p.size,
            prof.samples,
            reads.join(" ")
        );
    }
}
