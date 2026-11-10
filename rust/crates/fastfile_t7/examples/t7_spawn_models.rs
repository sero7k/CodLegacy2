//! Which static models' collision triangles a spawn's player box (32 x 32 x
//! 72, standing on the spawn's origin + 1) meets: `t7_spawn_models <map.ff>`.
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]));
    let map = fastfile_t7::map::T7Map::load(&src).expect("map");
    let clip = map.clip.as_ref().expect("clip");
    let mut spawns = Vec::new();
    for block in map.entities.split('}') {
        let field = |k: &str| block.lines().find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
        if field("classname").is_some_and(|c| c.contains("_spawn")) {
            if let Some(o) = field("origin") {
                let v: Vec<f32> = o.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    spawns.push([v[0], v[1], v[2]]);
                }
            }
        }
    }
    let place = |p: [f32; 3], sm: &fastfile_t7::map::ClipStaticModel| -> [f32; 3] { std::array::from_fn(|k| sm.origin[k] + (p[0] * sm.axis[0][k] + p[1] * sm.axis[1][k] + p[2] * sm.axis[2][k]) * sm.scale) };
    let mut hits: BTreeMap<(String, u32), usize> = BTreeMap::new();
    let mut held = 0;
    for s in &spawns {
        let (lo, hi) = ([s[0] - 15.0, s[1] - 15.0, s[2] + 1.0], [s[0] + 15.0, s[1] + 15.0, s[2] + 70.0]);
        let mut any = false;
        for sm in &clip.static_models {
            if (0..3).any(|k| (sm.origin[k] - s[k]).abs() > 2000.0) {
                continue;
            }
            for t in clip.model_collision.get(&sm.name).map(Vec::as_slice).unwrap_or(&[]) {
                if t.contents & 1 == 0 {
                    continue;
                }
                let bone = usize::try_from(t.bone).ok().and_then(|b| sm.bones.get(b));
                let c = t.corners.map(|p| place(bone.map_or(p, |m| fastfile_t7::map::bone_to_model(m, p)), sm));
                // Samples over the triangle.
                let inside = (0..=4).any(|i| (0..=4 - i).any(|j| {
                    let (u, v) = (i as f32 / 4.0, j as f32 / 4.0);
                    let p: [f32; 3] = std::array::from_fn(|k| c[0][k] + (c[1][k] - c[0][k]) * u + (c[2][k] - c[0][k]) * v);
                    (0..3).all(|k| p[k] > lo[k] && p[k] < hi[k])
                }));
                if inside {
                    *hits.entry((sm.name.clone(), t.contents)).or_default() += 1;
                    any = true;
                    break;
                }
            }
        }
        held += usize::from(any);
    }
    println!("{} spawns, {held} meet model collision", spawns.len());
    for ((n, c), k) in &hits {
        println!("  {k:4} {n} contents {c:#x}");
    }
    if let Some(name) = a.get(2) {
        for sm in clip.static_models.iter().filter(|sm| sm.name == *name).take(2) {
            println!("{name}: origin {:?} scale {} bones {:?}", sm.origin, sm.scale, sm.bones);
            let tris = clip.model_collision.get(name).map(Vec::as_slice).unwrap_or(&[]);
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for t in tris {
                for p in t.corners {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
            }
            println!("  {} tris, bone-space bounds {lo:?}..{hi:?}, bones used {:?}", tris.len(), tris.iter().map(|t| t.bone).collect::<std::collections::BTreeSet<_>>());
        }
    }
}
