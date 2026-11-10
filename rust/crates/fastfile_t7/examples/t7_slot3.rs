//! The G-buffer (technique slot 3) programs of a map's world materials:
//! `t7_slot3 <map.ff> <out dir> [max]` writes each distinct technique set's
//! vertex and pixel program as `<techset>_vs.dxbc` / `_ps.dxbc` (looked up
//! in the map zone, then in the other zones given with `-z <zone.ff>`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let out = std::path::Path::new(&a[2]);
    std::fs::create_dir_all(out).expect("out");
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);
    let w = fastfile_t7::walk::walk(&zone);
    let c = &w.capture;
    let mut seen = std::collections::BTreeMap::new();
    for g in &c.world.gfx_worlds {
        for m in g.surface_materials.iter().flatten() {
            *seen.entry(m.clone()).or_insert(0usize) += 1;
        }
    }
    let mut n = 0;
    let mut done = std::collections::HashSet::new();
    for (mat, count) in &seen {
        let Some(mr) = c.materials.iter().find(|r| &r.name == mat) else { continue };
        let Some(ts) = &mr.techset else { continue };
        if !done.insert(ts.clone()) {
            continue;
        }
        let rec = c.techsets.iter().find(|t| &t.name == ts);
        let progs = rec.and_then(|t| t.programs[3]);
        println!("{mat} ({count} surfaces): {ts} slot3 {:?}", progs);
        if let Some(info) = rec.and_then(|t| t.techniques[3].as_ref()) {
            for a in &info.args {
                println!("   arg ty {} loc {} size {} buf {} u {:08x} {:08x} lit {:?}", a.ty, a.location, a.size, a.buffer, a.u[0], a.u[1], a.literal);
            }
            if let Some(d) = info.decl {
                println!("   decl {:02x?}", &zone.image[d..d + 0x78]);
            }
            if let Some(sm) = info.state_map {
                println!("   state {:02x?}", &zone.image[sm..sm + 112]);
            }
        }
        if let Some(mr) = c.materials.iter().find(|r| &r.name == mat) {
            println!("   textures {:x?}", mr.textures);
            println!("   samplers {:02x?}", mr.sampler_defs);
            for (sl, n, d) in &mr.cbuffers { println!("   cb slot {sl} {n} {} bytes", d.len()); }
            for d in &mr.texture_defs { println!("   tdef {:02x?}", d); }
        }
        if let Some([vs, ps]) = progs {
            for (tag, p) in [("vs", vs), ("ps", ps)] {
                if let Some((at, len)) = p {
                    std::fs::write(out.join(format!("{}_{tag}.dxbc", ts.replace(['/', '\\'], "_"))), &zone.image[at..at + len]).ok();
                }
            }
            n += 1;
        }
        if n >= max {
            break;
        }
    }
}
