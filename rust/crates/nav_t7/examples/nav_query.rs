//! `nav_query <packfile.hkx | zone.ff> x y z x y z`: the path between two
//! points on the nav mesh, the faces it crosses and their polygons.
use nav_t7::NavMesh;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).expect("read");
    let mesh = match nav_t7::hkx::Packfile::parse(&bytes) {
        Ok(pf) => NavMesh::from_packfile(&pf).expect("mesh").expect("a nav mesh"),
        Err(_) => panic!("give a packfile (t7_havok_dump writes them)"),
    };
    let v: Vec<f32> = a[2..8].iter().map(|s| s.parse().expect("number")).collect();
    let (p, faces) = mesh.find_path_corridor([v[0], v[1], v[2]], [v[3], v[4], v[5]]).expect("path");
    for w in &p {
        println!("waypoint {:?}", w.point);
    }
    for f in faces {
        let poly: Vec<String> = mesh.polygon(f).iter().map(|p| format!("({:.0},{:.0},{:.0})", p[0], p[1], p[2])).collect();
        println!("face {f}: {}", poly.join(" "));
    }
}
