//! `t7_cs_bindings <core_patch.ff> <name filter>`: each matching compute
//! shader's generic D3D11 variant (path 2): thread groups, its RDEF bindings
//! (name, input type, bind point) and its arguments.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let f = a.get(2).cloned().unwrap_or_default();
    // T7_CS_MAKE=1: also make each as a pipeline and print its slots.
    let gpu = (std::env::var("T7_CS_MAKE").as_deref() == Ok("1")).then(|| render_h1::gpu::Gpu::headless().expect("gpu"));
    if let Some(gpu) = &gpu {
        let shaders = render_t7::compute::code_shaders(&z.image, &w.capture);
        let mut names: Vec<_> = shaders.keys().filter(|n| n.contains(&f)).cloned().collect();
        names.sort();
        for n in names {
            // T7_CS_WGSL=<dir>: also write each translation there.
            if let Ok(dir) = std::env::var("T7_CS_WGSL") {
                let cs = &shaders[&n];
                let c = dxbc_sm5::Container::parse(&cs.program).expect("dxbc");
                let p = dxbc_sm5::decode(c.shader).expect("decode");
                if let Ok(t) = dxbc_sm5::translate_program(&p, &c, &dxbc_sm5::Options::for_stage(dxbc_sm5::Stage::Compute)) {
                    std::fs::write(format!("{dir}/{n}.wgsl"), t.wgsl).expect("write");
                }
            }
            match render_t7::compute::Program::new(gpu, &shaders[&n], &[]) {
                Ok(p) => {
                    println!("made {n} threads {:?}", p.threads);
                    for s in &p.slots {
                        println!("  g{} b{} {:<32} {:?}", s.group, s.binding, s.name, s.kind);
                    }
                }
                Err(e) => println!("FAILED {e}"),
            }
        }
        return;
    }
    for c in w.capture.compute_shaders.iter().filter(|c| c.name.contains(&f)) {
        let Some(v) = c.variants.iter().find(|v| v.path == 2 && v.program.is_some()) else { continue };
        let (at, n) = v.program.unwrap();
        let prog = &z.image[at..at + n];
        println!("cs {} groups {:?}", c.name, v.groups);
        for b in render_t7::rdef::bindings(prog) {
            println!("  rdef {:<34} type {:>2} point {}", b.name, b.kind, b.point);
        }
        for a in &v.args {
            println!("  arg ty {:#06x} loc {} size {} buf {} u {:#x}", a.ty, a.location, a.size, a.buffer, a.u[0]);
        }
    }
}
