//! `t7_post_wgsl <core_patch.ff> <dir> <name>...`: a code technique set's
//! technique 5 programs translated (vertex and pixel WGSL into `dir`), with
//! their inputs, RDEF bindings and the technique's arguments, for the post chain (`render_t7::post`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    for n in &a[3..] {
        let Some(t) = w.capture.techsets.iter().find(|t| t.name.split('#').next() == Some(n.as_str())) else { println!("{n}: no techset"); continue };
        let Some(ti) = t.techniques[5].as_ref() else { continue };
        // The technique's arguments (type: 0x3xx sampler, 0x4xx code
        // constant, 0x5xx code texture, 0x6xx code buffer; low byte the stage).
        for g in &ti.args {
            println!("{n} arg ty {:#06x} location {} size {} buffer {} u {:#x?}", g.ty, g.location, g.size, g.buffer, g.u);
        }
        for (k, p) in ti.programs.iter().enumerate() {
            let Some((at, len)) = *p else { continue };
            let b = &z.image[at..at + len];
            let c = dxbc_sm5::Container::parse(b).expect("dxbc");
            let pr = dxbc_sm5::decode(c.shader).expect("decode");
            match dxbc_sm5::translate_program(&pr, &c, &dxbc_sm5::Options::for_stage(pr.stage)) {
                Ok(t) => std::fs::write(format!("{}/{n}.{}.wgsl", a[2], ["vs", "ps"][k]), t.wgsl).expect("write"),
                Err(e) => println!("{n} {k}: {e:?}"),
            }
            println!("{n} {k} inputs {:?} outputs {:?}", c.inputs.iter().map(|i| (&i.semantic, i.semantic_index)).collect::<Vec<_>>(), c.outputs.iter().map(|i| (&i.semantic, i.semantic_index)).collect::<Vec<_>>());
            for r in render_t7::rdef::bindings(b) {
                println!("   {} kind {} point {}", r.name, r.kind, r.point);
            }
        }
    }
}
