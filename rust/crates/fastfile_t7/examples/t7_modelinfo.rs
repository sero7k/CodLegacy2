fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    for name in &a[2..] {
        match w.capture.model(name).or_else(|| w.capture.model(&format!(",{name}"))) {
            Some(m) => println!("{name}: lods {:?} coll {:?}", m.lods.iter().map(|l| (l.mesh.clone(), l.materials.len())).collect::<Vec<_>>(), m.collision_mesh),
            None => println!("{name}: not in zone"),
        }
    }
}
