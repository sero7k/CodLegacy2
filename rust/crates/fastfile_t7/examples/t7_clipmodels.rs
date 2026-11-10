//! The clip map's static models (the ones the world collides with) and
//! whether each is built: `t7_clipmodels <map.ff>`.
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]));
    let map = fastfile_t7::map::T7Map::load(&src).expect("map");
    for r in map.report.iter().filter(|r| r.contains("static models") || r.contains("collision")) {
        println!("{r}");
    }
    let Some(clip) = &map.clip else { return };
    let built: std::collections::HashSet<&str> = map.models.iter().map(|m| m.name.as_str()).collect();
    let mut missing: BTreeMap<&str, usize> = BTreeMap::new();
    let mut contents: BTreeMap<u32, usize> = BTreeMap::new();
    for sm in &clip.static_models {
        *contents.entry(sm.contents).or_default() += 1;
        if !built.contains(sm.name.as_str()) {
            *missing.entry(sm.name.as_str()).or_default() += 1;
        }
    }
    println!("clip static models {}, contents {contents:x?}", clip.static_models.len());
    println!("not built {} ({} kinds):", missing.values().sum::<usize>(), missing.len());
    for (n, c) in &missing {
        println!("  {c:4} {n}");
    }
    // What collision data each clip static model's XModel carries in the
    // zone (not streamed): collision surfaces, collmaps, a collision LOD.
    let w = fastfile_t7::walk::walk(&fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone"));
    let mut kinds: BTreeMap<(bool, bool, bool), usize> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for sm in &clip.static_models {
        if !seen.insert(sm.name.clone()) {
            continue;
        }
        let m = w.capture.model(&sm.name).or_else(|| w.capture.model(&format!(",{}", sm.name)));
        let k = m.map_or((false, false, false), |m| (m.coll_surfs_at.is_some_and(|c| c.1 > 0), m.collmaps_at.is_some_and(|c| c.1 > 0), m.collision_mesh.is_some()));
        *kinds.entry(k).or_default() += 1;
    }
    println!("clip static model kinds by (coll surfs, collmaps, collision LOD): {kinds:?}");
    // The triangle convention: s = dot(svec.xyz, p) - sign * svec.w; the
    // vertices are (s, t) = (0,0), (1,0), (0,1) on the plane. The right
    // sign keeps every vertex inside the surface's bounds.
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let img = &z.image;
    let f = |o: usize| f32::from_le_bytes(img[o..o + 4].try_into().unwrap());
    let mut score = [0usize; 4];
    let mut total = 0usize;
    for m in w.capture.models.iter().take(400) {
        for (si, cs) in m.coll_surfs.iter().enumerate() {
            let Some(at) = cs.tris_at else { continue };
            let Some((st, _)) = m.coll_surfs_at else { continue };
            let sb = st + 48 * si + 12;
            let (lo, hi) = ([f(sb), f(sb + 4), f(sb + 8)], [f(sb + 12), f(sb + 16), f(sb + 20)]);
            for t in 0..cs.tri_count.min(64) {
                let o = at + 48 * t;
                let v: Vec<f32> = (0..12).map(|k| f(o + 4 * k)).collect();
                for (k, (sp, sd)) in [(1.0f32, 1.0f32), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].into_iter().enumerate() {
                    let ok = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)].iter().all(|&(sv, tv)| {
                        match fastfile_t7::map::solve3([[v[0], v[1], v[2]], [v[4], v[5], v[6]], [v[8], v[9], v[10]]], [sp * v[3], sd * v[7] + sv, sd * v[11] + tv]) {
                            Some(p) => (0..3).all(|i| p[i] >= lo[i] - 0.5 && p[i] <= hi[i] + 0.5),
                            None => false,
                        }
                    });
                    if ok {
                        score[k] += 1;
                    }
                }
                total += 1;
            }
        }
    }
    println!("triangle convention (plane sign, s/t sign): {score:?} of {total}");
    if let Some(m) = w.capture.models.iter().find(|m| m.name.contains(a.get(2).map_or("crate", |s| s.as_str())) && !m.coll_surfs.is_empty()) {
        println!("model {} surfs {}", m.name, m.coll_surfs.len());
        let (st, _) = m.coll_surfs_at.unwrap();
        for (si, cs) in m.coll_surfs.iter().enumerate().take(2) {
            let sb = st + 48 * si;
            println!(" surf {si}: raw {:?} bone {} contents {:#x} flags {:#x} tris {}", (0..12).map(|k| f(sb + 4 * k)).collect::<Vec<_>>(), cs.bone, cs.contents, cs.surface_flags, cs.tri_count);
            if let Some(at) = cs.tris_at {
                for t in 0..cs.tri_count.min(4) {
                    println!("   tri {t}: {:?}", (0..12).map(|k| f(at + 48 * t + 4 * k)).collect::<Vec<_>>());
                }
            }
        }
    }
}
