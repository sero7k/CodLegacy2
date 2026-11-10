//! `t7_oob_check <map.ff> [x y z]...`: a Black Ops III map's out-of-bounds
//! volumes (`trigger_out_of_bounds`: their trigger model's hulls and slabs,
//! as `sim::t6::triggers` tests them) in world space, and which spawn points
//! (and the given points) a standing player's box there touches. A spawn
//! point inside one means the volume test is wrong (BO3 never spawns a
//! player out of bounds).
fn blocks(text: &str) -> Vec<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<(String, String)>> = None;
    for line in text.lines() {
        let l = line.trim();
        if l == "{" {
            cur = Some(Vec::new());
        } else if l == "}" {
            if let Some(c) = cur.take() {
                out.push(c);
            }
        } else if let Some(c) = cur.as_mut() {
            let parts: Vec<&str> = l.split('"').collect();
            if parts.len() >= 4 {
                c.push((parts[1].to_owned(), parts[3].to_owned()));
            }
        }
    }
    out
}

fn get<'a>(b: &'a [(String, String)], k: &str) -> Option<&'a str> {
    b.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
}

fn vec3(s: &str) -> [f32; 3] {
    let v: Vec<f32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)]
}

/// The player box (15 x 15 x 0..70 round his origin) against the hulls.
fn touches(origin: [f32; 3], hulls: &[([f32; 3], [f32; 3])], slabs: Option<&Vec<Vec<([f32; 3], f32, f32)>>>, p: [f32; 3]) -> bool {
    let (mins, maxs) = ([p[0] - 15.0, p[1] - 15.0, p[2]], [p[0] + 15.0, p[1] + 15.0, p[2] + 70.0]);
    let mid: [f32; 3] = std::array::from_fn(|i| (mins[i] + maxs[i]) * 0.5 - origin[i]);
    let half: [f32; 3] = std::array::from_fn(|i| (maxs[i] - mins[i]) * 0.5);
    hulls.iter().enumerate().any(|(k, (m, h))| {
        (0..3).all(|i| (mid[i] - m[i]).abs() <= half[i] + h[i])
            && slabs.and_then(|s| s.get(k)).is_none_or(|s| {
                s.iter().all(|&(dir, at, width)| {
                    let reach: f32 = (0..3).map(|i| dir[i].abs() * half[i]).sum();
                    let along: f32 = (0..3).map(|i| dir[i] * mid[i]).sum();
                    (along - at).abs() <= width + reach
                })
            })
    })
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let ents = blocks(&map.entities);
    let mut extra: Vec<[f32; 3]> = Vec::new();
    for c in a[2..].chunks(3) {
        if c.len() == 3 {
            extra.push([c[0].parse().unwrap_or(0.0), c[1].parse().unwrap_or(0.0), c[2].parse().unwrap_or(0.0)]);
        }
    }
    let spawns: Vec<(String, [f32; 3])> = ents
        .iter()
        .filter_map(|b| {
            let c = get(b, "classname")?;
            (c.starts_with("mp_") && c.contains("_spawn")).then(|| (c.to_owned(), vec3(get(b, "origin").unwrap_or("0 0 0"))))
        })
        .collect();
    let mut volumes = 0;
    let mut bad = 0;
    for b in &ents {
        let Some(class) = get(b, "classname").filter(|c| c.contains("out_of_bounds")) else { continue };
        volumes += 1;
        let origin = vec3(get(b, "origin").unwrap_or("0 0 0"));
        let model = get(b, "model").unwrap_or("");
        let Some(n) = model.strip_prefix('?').and_then(|m| m.parse::<usize>().ok()) else {
            println!("{class} model {model:?} at {origin:?}: not a trigger model (radius {:?})", get(b, "radius"));
            continue;
        };
        let hulls = map.trigger_models.get(n).cloned().unwrap_or_default();
        let slabs = map.trigger_slabs.get(n);
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for (m, h) in &hulls {
            for i in 0..3 {
                lo[i] = lo[i].min(origin[i] + m[i] - h[i]);
                hi[i] = hi[i].max(origin[i] + m[i] + h[i]);
            }
        }
        let nslabs: usize = slabs.map_or(0, |s| s.iter().map(Vec::len).sum());
        println!(
            "{class} {model} at ({:.0} {:.0} {:.0}): {} hulls, {nslabs} slabs, world ({:.0} {:.0} {:.0})..({:.0} {:.0} {:.0})",
            origin[0], origin[1], origin[2], hulls.len(), lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
        );
        for (name, p) in &spawns {
            if touches(origin, &hulls, slabs, *p) {
                bad += 1;
                println!("    SPAWN INSIDE: {name} at ({:.0} {:.0} {:.0})", p[0], p[1], p[2]);
            }
        }
        for p in &extra {
            println!("    point ({:.0} {:.0} {:.0}): {}", p[0], p[1], p[2], if touches(origin, &hulls, slabs, *p) { "INSIDE" } else { "outside" });
        }
    }
    println!("{volumes} out-of-bounds volumes, {} spawn points, {bad} spawn points inside one", spawns.len());
}
