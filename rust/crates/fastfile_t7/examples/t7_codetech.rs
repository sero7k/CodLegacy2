//! `t7_codetech <zone.ff> <name>...`: a code technique set's technique 5
//! (its programs' sizes, vertex declaration and arguments), for the post
//! chain (`render_t7::post`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    for n in &a[2..] {
        for m in w.capture.materials.iter().filter(|m| m.techset.as_deref().and_then(|t| t.trim_start_matches(',').split('#').next()) == Some(n.as_str())) {
            let s: Vec<String> = m.sampler_defs.iter().map(|d| format!("{:08x}:{:#06x}", u32::from_le_bytes(d[0..4].try_into().unwrap()), u16::from_le_bytes([d[4], d[5]]) & 0x3fff)).collect();
            println!("material {} techset {:?} samplers {s:?} textures {:?}", m.name, m.techset, m.textures);
        }
        for t in w.capture.techsets.iter().filter(|t| t.name.split('#').next() == Some(n.as_str())) {
            for k in [5usize, 6] {
                let Some(ti) = &t.techniques[k] else { continue };
                let decl = ti.decl.and_then(|d| z.image.get(d..d + 0x10)).map(|b| format!("{b:02x?}")).unwrap_or_default();
                println!("{} tech {k} vs {:?} ps {:?} decl {decl} state {:?}", t.name, ti.programs[0].map(|p| p.1), ti.programs[1].map(|p| p.1), ti.state_map);
                for g in &ti.args {
                    println!("  ty {:#06x} loc {} size {} buf {} u {:#x} {:#x} lit {:?}", g.ty, g.location, g.size, g.buffer, g.u[0], g.u[1], g.literal);
                }
            }
        }
    }
}
