//! `t7_code <zone.ff> [filter]`: the zone's compute shaders (variants and
//! program sizes) and technique sets (techniques with programs).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let f = a.get(2).cloned().unwrap_or_default();
    for c in w.capture.compute_shaders.iter().filter(|c| c.set.contains(&f) || c.name.contains(&f)) {
        let v: Vec<String> = c.variants.iter().map(|v| format!("{}:{}", v.path, v.program.map_or(0, |p| p.1))).collect();
        println!("cs {} / {} [{}]", c.set, c.name, v.join(" "));
        // (With T7_CODE_ARGS: each variant's thread groups and arguments.)
        if std::env::var("T7_CODE_ARGS").is_ok() {
            for (i, var) in c.variants.iter().enumerate().filter(|(_, v)| v.program.is_some()) {
                println!("  variant {i} path {} groups {:?}", var.path, var.groups);
                for a in &var.args {
                    println!("    ty {:#06x} loc {} size {} buf {} u {:#x} {:#x} lit {:?}", a.ty, a.location, a.size, a.buffer, a.u[0], a.u[1], a.literal);
                }
            }
        }
    }
    for t in w.capture.techsets.iter().filter(|t| t.name.contains(&f)) {
        let s: Vec<usize> = (0..12).filter(|&i| t.techniques[i].is_some()).collect();
        println!("ts {} techniques {s:?}", t.name);
    }
}
