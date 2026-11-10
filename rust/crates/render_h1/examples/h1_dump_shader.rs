//! Writes one shader program from the zones as WGSL (pixel, vertex or
//! compute), or lists the programs of a kind whose name holds a word.
//!
//! `cargo run -p render_h1 --example h1_dump_shader -- <ps|vs|cs> <name> <zone.ff>...`
//! `cargo run -p render_h1 --example h1_dump_shader -- list-<ps|vs|cs> <word> <zone.ff>...`

use fastfile_h1::AssetType;
use render_h1::pool::Pool;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (list, kind) = match a[0].strip_prefix("list-") {
        Some(k) => (true, k),
        None => (false, a[0].as_str()),
    };
    let (ty, stage) = match kind {
        "vs" => (AssetType::VertexShader, dxbc_sm5::Stage::Vertex),
        "cs" => (AssetType::ComputeShader, dxbc_sm5::Stage::Compute),
        _ => (AssetType::PixelShader, dxbc_sm5::Stage::Pixel),
    };
    if list {
        for p in &a[2..] {
            let zone = fastfile_h1::Zone::open(std::path::Path::new(p)).expect("zone");
            let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
            eprintln!(
                "{p}: {} programs, walk stop {stop:?}",
                cap.shader_programs.len()
            );
            for (t, name, span) in &cap.shader_programs {
                if *t == ty && name.contains(a[1].as_str()) {
                    println!("{name} ({} bytes)", span.len);
                }
            }
        }
        return;
    }
    let mut pool = Pool::default();
    for p in &a[2..] {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(p)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        pool.add(&cap, &zone.image);
    }
    let b = pool.program(ty, &a[1]).expect("program");
    // H1_RAW=<path>: the program's bytes written there as they are.
    if let Ok(p) = std::env::var("H1_RAW") {
        std::fs::write(p, b).expect("write");
        return;
    }
    match dxbc_sm5::translate(b, &dxbc_sm5::Options::for_stage(stage)) {
        Ok(t) => println!("{}", t.wgsl),
        Err(e) => eprintln!("{e:?}"),
    }
}
