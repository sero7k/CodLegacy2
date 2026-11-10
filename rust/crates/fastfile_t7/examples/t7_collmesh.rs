//! Each model's LOD meshes and its collision LOD mesh: `t7_collmesh <zone.ff>
//! [name substring]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let w = fastfile_t7::walk::walk(&z);
    for m in w.capture.models.iter().filter(|m| m.name.contains(&want)) {
        let lods: Vec<String> = m
            .lods
            .iter()
            .map(|l| {
                let tris = w
                    .capture
                    .mesh(l.mesh.trim_start_matches(','))
                    .map(|me| {
                        me.surfaces
                            .iter()
                            .map(|s| s.tri_count as usize)
                            .sum::<usize>()
                    })
                    .unwrap_or(0);
                format!("{}({tris})", l.mesh)
            })
            .collect();
        let coll = m.collision_mesh.as_ref().map(|c| {
            let tris = w
                .capture
                .mesh(c.trim_start_matches(','))
                .map(|me| {
                    me.surfaces
                        .iter()
                        .map(|s| s.tri_count as usize)
                        .sum::<usize>()
                })
                .unwrap_or(0);
            format!("{c}({tris})")
        });
        println!(
            "{} lods {:?} coll {:?} phys {:?}",
            m.name, lods, coll, m.phys_preset
        );
    }
}
