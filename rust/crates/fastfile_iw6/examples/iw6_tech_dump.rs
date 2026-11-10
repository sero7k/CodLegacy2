//! Lists a Ghosts technique set's techniques, passes and shader arguments and
//! writes each pass's vertex and pixel programs (DXBC) to a directory, for
//! `dxbc_sm5`'s `dxbc2wgsl`. `iw6_tech_dump <techsets_*.ff> <techset name part> <out dir> [technique name part]`
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: iw6_tech_dump <zone.ff> <techset part> <out dir> [technique part]");
        return;
    }
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&a[1])).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    // Programs by name from the sibling technique-set zones too (a pass may
    // reference a program another zone holds).
    let mut programs: HashMap<String, Vec<u8>> = HashMap::new();
    let dir = std::path::Path::new(&a[1]).parent().unwrap().to_owned();
    for n in ["techsets_common", "techsets_common_core_mp", "techsets_code_post_gfx_mp"] {
        if let Ok(z) = fastfile_iw6::Zone::open(&dir.join(format!("{n}.ff"))) {
            let (c, _) = fastfile_iw6::walk::walk_zone(&z);
            for (id, span) in &c.shader_programs {
                let name = c.name_of(*id);
                if span.len > 0 && !name.starts_with(',') {
                    if let Some(b) = z.image.get(span.pos..span.pos + span.len) {
                        programs.entry(name.to_owned()).or_insert_with(|| b.to_vec());
                    }
                }
            }
        }
    }
    for (id, span) in &cap.shader_programs {
        let name = cap.name_of(*id);
        if span.len > 0 && !name.starts_with(',') {
            if let Some(b) = zone.image.get(span.pos..span.pos + span.len) {
                programs.insert(name.to_owned(), b.to_vec());
            }
        }
    }
    let out = std::path::Path::new(&a[3]);
    std::fs::create_dir_all(out).ok();
    let tech_filter = a.get(4).cloned().unwrap_or_default();
    let mut written: HashMap<u32, String> = HashMap::new();
    let mut shown = 0;
    for ts in cap.techsets.iter().filter(|t| t.name.trim_start_matches(',') == a[2] || (a[2].ends_with('*') && t.name.contains(&a[2][..a[2].len() - 1]))) {
        println!("TECHSET {}", ts.name);
        for (slot, tid) in &ts.technique_ids {
            let t = &cap.techniques[*tid as usize];
            if !tech_filter.is_empty() && !t.name.contains(&tech_filter) {
                continue;
            }
            println!(" technique slot {slot} {} flags {:#x}, {} passes", t.name, t.flags, t.passes.len());
            for (pi, p) in t.passes.iter().enumerate() {
                println!("  pass {pi} arg counts {:?} sizes {:?}", p.arg_counts, p.arg_sizes);
                for (stage, sh) in [("vs", p.vertex_shader), ("ps", p.pixel_shader)] {
                    if let Some(id) = sh {
                        let pname = cap.name_of(id).trim_start_matches(',').to_owned();
                        if let Some(bytes) = programs.get(&pname) {
                            let name = format!("{}_{}_p{}_{}.dxbc", t.name, slot, pi, stage);
                            std::fs::write(out.join(&name), bytes).ok();
                            let _ = &pname;
                            written.insert(id.index, name.clone());
                            println!("   {stage}: {} bytes -> {name}", bytes.len());
                        }
                    }
                }
                for g in &p.args {
                    let (idx, row, rows) = g.code_const();
                    match g.ty {
                        0 => println!("   arg code-const shader {} dest {} index {} row {} rows {}", g.shader, g.dest, idx, row, rows),
                        1 | 2 => println!("   arg code-tex/sampler ty {} shader {} dest {} src {}", g.ty, g.shader, g.dest, u32::from_le_bytes([g.u[0], g.u[1], g.u[2], g.u[3]])),
                        3 | 5 | 6 => println!("   arg material ty {} shader {} dest {} hash {:#x}", g.ty, g.shader, g.dest, u32::from_le_bytes([g.u[0], g.u[1], g.u[2], g.u[3]])),
                        _ => println!("   arg literal shader {} dest {} {:?}", g.shader, g.dest, g.literal),
                    }
                }
            }
            shown += 1;
        }
    }
    println!("{shown} techniques");
    // `IW6_MAT_ZONE=<zone.ff> IW6_MAT=<name part>`: that zone's materials' constants and techset.
    if let (Ok(zp), Ok(part)) = (std::env::var("IW6_MAT_ZONE"), std::env::var("IW6_MAT")) {
        let z = fastfile_iw6::Zone::open(std::path::Path::new(&zp)).expect("open material zone");
        let (c, _) = fastfile_iw6::walk::walk_zone(&z);
        for m in c.materials.iter().filter(|m| m.name.contains(&part)) {
            let ts = m.techset.map(|t| c.name_of(t).to_owned()).unwrap_or_default();
            println!("MATERIAL {} techset {ts} sort {} flags {:#x} region {}", m.name, m.sort_key, m.state_flags, m.camera_region);
            for k in &m.constants {
                let n = &k.name;
                println!("   const {n} {:#x} {:?}", k.name_hash, k.literal);
            }
        }
    }
}
