//! A BO3 clip map's contents census: `t7_contents <map.ff>` prints how many
//! brushes, mesh triangles (by their AABB leaf's material) and model
//! collision triangles carry each contents value, and the materials.
fn main() {
    use std::collections::BTreeMap;
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let c = map.clip.as_ref().expect("clip");
    let mut brushes: BTreeMap<u32, usize> = BTreeMap::new();
    for b in &c.brushes {
        *brushes.entry(b.contents).or_default() += 1;
    }
    println!("brush contents: {brushes:x?}");
    // Each brush's corners against its planes: a corner outside a plane by
    // more than 0.5, and planes no corner lies on (a side that cuts nothing
    // the corners span).
    let (mut with_verts, mut bad_brushes, mut bad_planes, mut sides) = (0usize, 0usize, 0usize, 0usize);
    let mut shown = 0;
    for (i, b) in c.brushes.iter().enumerate() {
        if b.verts.is_empty() {
            continue;
        }
        with_verts += 1;
        let mut bad = false;
        for (k, p) in b.planes.iter().enumerate() {
            let d: Vec<f32> = b.verts.iter().map(|v| v[0] * p[0] + v[1] * p[1] + v[2] * p[2] - p[3]).collect();
            let out = d.iter().copied().fold(f32::MIN, f32::max);
            let on = d.iter().any(|x| x.abs() < 0.5);
            if k >= 6 {
                sides += 1;
            }
            if out > 0.5 || (k >= 6 && !on) {
                bad = true;
                if k >= 6 {
                    bad_planes += 1;
                }
                if shown < 12 {
                    shown += 1;
                    println!("brush {i} plane {k} {p:?}: corners out by {out:.2}, on it {on}; {} corners, {} planes", b.verts.len(), b.planes.len());
                }
            }
        }
        if bad {
            bad_brushes += 1;
        }
    }
    println!("brush corners: {with_verts} of {} brushes have them; {bad_brushes} disagree with their planes ({bad_planes} of {sides} extra sides)", c.brushes.len());
    let mut mesh: BTreeMap<u32, usize> = BTreeMap::new();
    for n in &c.aabb_trees {
        if n.child_count != 0 {
            continue;
        }
        let cnt = c.partitions.get(n.u as usize).map_or(0, |p| usize::from(p.tri_count));
        let cf = c.materials.get(usize::from(n.material)).map_or(u32::MAX, |m| m.2);
        *mesh.entry(cf).or_default() += cnt;
    }
    println!("mesh triangle contents (by leaf material): {mesh:x?}");
    // Walkable edge bits: how many set, of the up-facing triangles' edges.
    let bit = |i: usize| c.walkable.get(i / 8).is_some_and(|b| b >> (i % 8) & 1 != 0);
    let (mut up, mut up_set, mut all_set) = (0usize, 0usize, 0usize);
    for (t, tri) in c.tris.iter().enumerate() {
        let v = |k: usize| c.verts.get(tri[k] as usize).copied().unwrap_or_default();
        let (a, b, cc) = (v(0), v(1), v(2));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]];
        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
        let set = (0..3).filter(|&k| bit(3 * t + k)).count();
        all_set += set;
        if n[2] / l.abs() < -0.7 || n[2] / l > 0.7 {
            up += 3;
            up_set += set;
        }
    }
    println!("walkable edge bits: {} bytes for {} triangles; set {all_set} of {}; of floor triangles' edges {up_set} of {up}", c.walkable.len(), c.tris.len(), 3 * c.tris.len());
    let mut models: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for sm in &c.static_models {
        for t in c.model_collision.get(&sm.name).map(Vec::as_slice).unwrap_or(&[]) {
            *models.entry((t.contents, sm.contents)).or_default() += 1;
        }
    }
    println!("model triangles (tri contents, static model contents): {models:x?}");
    for (i, (n, s, cf)) in c.materials.iter().enumerate() {
        println!("material {i}: {n} surface {s:#x} contents {cf:#x}");
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for b in &c.brushes {
        for k in 0..3 {
            lo[k] = lo[k].min(-b.planes.get(2 * k + 1).map_or(0.0, |p| p[3]));
            hi[k] = hi[k].max(b.planes.get(2 * k).map_or(0.0, |p| p[3]));
        }
    }
    println!("brush box extent {lo:?}..{hi:?}");
    let mut big: Vec<(usize, u32, [f32; 6])> = Vec::new();
    for (i, b) in c.brushes.iter().enumerate() {
        let g = |k: usize| b.planes.get(k).map_or(0.0, |p| p[3]);
        if (g(0) + g(1)).abs() > 20000.0 || (g(4) + g(5)).abs() > 20000.0 || (g(2) + g(3)).abs() > 20000.0 {
            big.push((i, b.contents, [-g(1), -g(3), -g(5), g(0), g(2), g(4)]));
        }
    }
    println!("brushes over 20000 units: {} {:?}", big.len(), &big[..big.len().min(12)]);
}
