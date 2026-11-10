//! `t7_surf_at <map.ff> <x|x> <y|x> [radius] [step]`: what the scene draws
//! straight below points of the map, CPU only (no GPU): a vertical ray down
//! at each point of a grid around (x, y) hits world surfaces and placed
//! static models; per point the hits from the top, and per material how
//! many grid points it is the topmost surface of (the floor a view from
//! above sees first), with its technique set, list, sort key and pictures.
//! For finding which surface draws a wrong floor (Nuk3town's cyan plaza)
//! or which floor is missing. Also: the map entities with a model in the
//! square, the static model placements in it whose mesh was not built
//! (Nuk3town's road and sidewalk stream from the map's package), a floor
//! map (one character per grid point: the topmost surface; sky and vista
//! left out unless `T7_SURF_SKY=1`; `T7_SURF_BELOW=z` leaves out what is
//! above z). `T7_SURF_ALL=1` prints every point's hits, `T7_FIND=<text>`
//! where the static models named so are placed, `T7_DRAWS=<text>` the
//! draws of the materials named so (bounds, list, sort key, state).
//! The point defaults to the middle of the team deathmatch spawns.
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    let (mins, maxs) = (scene.map.mins, scene.map.maxs);
    println!("map bounds {mins:?} .. {maxs:?}");
    for l in scene.report.iter().filter(|l| l.starts_with("world") || l.starts_with("static models")) {
        println!("scene: {l}");
    }
    for (why, n) in &scene.not_drawn {
        println!("not drawn: {n} {why}");
    }
    // The middle of the map's team deathmatch spawns (the play area; the
    // bounds take in the sky and vista): the default point.
    let mut spawns = Vec::new();
    for block in scene.map.entities.split('}') {
        if !block.contains("mp_tdm_spawn") {
            continue;
        }
        let o = block.split('\n').find_map(|l| l.trim().strip_prefix("\"origin\" \"").map(|v| v.trim_end_matches('"').to_owned()));
        if let Some(v) = o.map(|o| o.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>())
            && v.len() == 3
        {
            spawns.push(glam::Vec3::new(v[0], v[1], v[2]));
        }
    }
    let mid = spawns.iter().copied().sum::<glam::Vec3>() / spawns.len().max(1) as f32;
    println!("{} tdm spawns, middle {mid}", spawns.len());
    // `T7_FIND=<text>`: where the static models whose name has the text are.
    if let Ok(find) = std::env::var("T7_FIND") {
        for d in scene.draws.iter().filter(|d| d.group == 1) {
            let Some(name) = scene.model_names.get(d.source as usize) else { continue };
            if !name.to_lowercase().contains(&find.to_lowercase()) {
                continue;
            }
            for i in d.instance..d.instance + d.instances {
                if let Some(m) = scene.instances.get(i as usize).and_then(|ins| scene.bones.get(ins[3] as usize)) {
                    println!("model {name} at ({:.0}, {:.0}, {:.0}) material {}", m[3], m[7], m[11], scene.materials[d.material].name);
                }
            }
        }
    }
    // `T7_DRAWS=<text>`: the draws whose material has the text: bounds,
    // triangles, and the bounds of their vertices as read.
    if let Ok(find) = std::env::var("T7_DRAWS") {
        for (di, d) in scene.draws.iter().enumerate() {
            let m = &scene.materials[d.material];
            if !m.name.to_lowercase().contains(&find.to_lowercase()) {
                continue;
            }
            let src = if d.group == 3 { &scene.world } else { &scene.models };
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for k in 0..d.index_count as usize {
                let Some(&ix) = src.indices.get(d.first_index as usize + k) else { break };
                if let Some(p) = render_t7::winding::pos(&src.positions, (d.base_vertex + i32::from(ix)) as usize) {
                    for c in 0..3 {
                        lo[c] = lo[c].min(p[c]);
                        hi[c] = hi[c].max(p[c]);
                    }
                }
            }
            println!("draw {di} {} ({}) group {} list {} range {} flags {:#x}: {} triangles, bounds {:?}..{:?}, vertices {lo:?}..{hi:?}, fwd {} gb {} pre {} sort {:016x}", m.name, m.techset, d.group, d.list, d.range, d.flags, d.index_count / 3, d.mins, d.maxs, m.forward.is_some(), m.technique.is_some(), m.prepass.is_some(), m.sort_key);
            if let Some(t) = m.forward.as_ref().or(m.technique.as_ref()) {
                println!("    state {:?}", t.state);
            }
        }
    }
    let cx: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(mid.x);
    let cy: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(mid.y);
    let r: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(400.0);
    let step: f32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(50.0);
    let all = std::env::var("T7_SURF_ALL").is_ok();
    let ident = {
        let mut m = [0.0f32; 16];
        (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
        m
    };
    // Triangles (world space) of every draw that overlaps the square.
    struct Tri {
        p: [[f32; 3]; 3],
        draw: usize,
    }
    let mut tris = Vec::new();
    for (di, d) in scene.draws.iter().enumerate() {
        let src = if d.group == 3 { &scene.world } else { &scene.models };
        let placements: Vec<[f32; 16]> = if d.group == 3 {
            vec![ident]
        } else {
            (d.instance..d.instance + d.instances)
                .filter_map(|i| scene.instances.get(i as usize))
                .map(|ins| scene.bones.get(ins[3] as usize).copied().unwrap_or(ident))
                .collect()
        };
        for m in &placements {
            let to = |p: [f32; 3]| -> [f32; 3] { std::array::from_fn(|r| m[4 * r] * p[0] + m[4 * r + 1] * p[1] + m[4 * r + 2] * p[2] + m[4 * r + 3]) };
            for t in 0..(d.index_count / 3) as usize {
                let i0 = d.first_index as usize + 3 * t;
                let Some(ix) = src.indices.get(i0..i0 + 3) else { break };
                let v = |k: usize| render_t7::winding::pos(&src.positions, (d.base_vertex + i32::from(ix[k])) as usize);
                let (Some(p0), Some(p1), Some(p2)) = (v(0), v(1), v(2)) else { continue };
                let p = [to(p0), to(p1), to(p2)];
                let (lx, hx) = (p.iter().map(|q| q[0]).fold(f32::MAX, f32::min), p.iter().map(|q| q[0]).fold(f32::MIN, f32::max));
                let (ly, hy) = (p.iter().map(|q| q[1]).fold(f32::MAX, f32::min), p.iter().map(|q| q[1]).fold(f32::MIN, f32::max));
                if hx < cx - r || lx > cx + r || hy < cy - r || ly > cy + r {
                    continue;
                }
                tris.push(Tri { p, draw: di });
            }
        }
    }
    // Map entities with a model in the square (script models and brush
    // models are drawn by the game from the entity, not the static list).
    for block in scene.map.entities.split('}') {
        let val = |k: &str| block.split('\n').find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
        let (Some(model), Some(o)) = (val("model"), val("origin")) else { continue };
        let v: Vec<f32> = o.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() == 3 && (v[0] - cx).abs() <= r && (v[1] - cy).abs() <= r {
            println!("entity in the square: {} model {model} at {o} targetname {:?} script_noteworthy {:?}", val("classname").unwrap_or_default(), val("targetname"), val("script_noteworthy"));
        }
    }
    // Placements whose model was not built, in the square: a floor model
    // missing here leaves a hole.
    let mut unbuilt: BTreeMap<String, (usize, String, Vec<[f32; 3]>)> = BTreeMap::new();
    for (name, o, why) in &scene.unbuilt {
        if (o[0] - cx).abs() <= r && (o[1] - cy).abs() <= r {
            let e = unbuilt.entry(name.clone()).or_insert((0, why.clone(), Vec::new()));
            e.0 += 1;
            if e.2.len() < 4 {
                e.2.push(*o);
            }
        }
    }
    for (name, (n, why, at)) in &unbuilt {
        println!("unbuilt in the square: {name} x{n} at {at:?}: {}", why.chars().take(200).collect::<String>());
    }
    println!("{} triangles over the square ({cx}, {cy}) +-{r}", tris.len());
    let mut top: BTreeMap<String, usize> = BTreeMap::new();
    let mut any: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_draw: BTreeMap<String, usize> = BTreeMap::new();
    let n = (2.0 * r / step) as i32;
    let keep_sky = std::env::var("T7_SURF_SKY").is_ok();
    let below: f32 = std::env::var("T7_SURF_BELOW").ok().and_then(|v| v.parse().ok()).unwrap_or(f32::MAX);
    let mut legend: BTreeMap<String, usize> = BTreeMap::new();
    let mut rows = Vec::new();
    for j in 0..=n {
        let mut row = String::new();
        for i in 0..=n {
            let (x, y) = (cx - r + i as f32 * step, cy - r + j as f32 * step);
            let mut hits: Vec<(f32, usize, bool)> = Vec::new();
            for t in &tris {
                let [a, b, c] = t.p;
                let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if d.abs() < 1e-6 {
                    continue;
                }
                let l0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
                let l1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
                let l2 = 1.0 - l0 - l1;
                if l0 < 0.0 || l1 < 0.0 || l2 < 0.0 {
                    continue;
                }
                let z = l0 * a[2] + l1 * b[2] + l2 * c[2];
                // Facing up (counter-clockwise seen from above) or down.
                hits.push((z, t.draw, d > 0.0));
            }
            hits.sort_by(|p, q| q.0.total_cmp(&p.0));
            hits.dedup_by(|p, q| p.1 == q.1 && (p.0 - q.0).abs() < 0.01);
            // The sky dome and the vista (far below and around the play
            // area) are under every point: left out of the floor map
            // (`T7_SURF_SKY=1` keeps them).
            if !keep_sky {
                hits.retain(|h| {
                    let n = &scene.materials[scene.draws[h.1].material].name;
                    !n.contains("skybox") && !n.contains("vista")
                });
            }
            // Above the eye height asked (`T7_SURF_BELOW=z`): roofs left out.
            hits.retain(|h| h.0 <= below);
            let ch = match hits.first() {
                Some(h) => {
                    let n = scene.materials[scene.draws[h.1].material].name.clone();
                    let k = legend.len();
                    let e = legend.entry(n).or_insert(k);
                    char::from(if *e < 26 { b'a' + *e as u8 } else if *e < 52 { b'A' + (*e - 26) as u8 } else { b'#' })
                }
                None => '.',
            };
            row.push(ch);
            if let Some(h) = hits.first() {
                *top.entry(scene.materials[scene.draws[h.1].material].name.clone()).or_default() += 1;
            } else {
                *top.entry("(nothing)".into()).or_default() += 1;
            }
            for h in &hits {
                let d = &scene.draws[h.1];
                *any.entry(scene.materials[d.material].name.clone()).or_default() += 1;
                seen_draw.entry(scene.materials[d.material].name.clone()).or_insert(h.1);
            }
            if all {
                let s: Vec<String> = hits
                    .iter()
                    .take(6)
                    .map(|h| {
                        let d = &scene.draws[h.1];
                        format!("z {:.1} {}{} {}", h.0, if h.2 { "" } else { "(down) " }, scene.materials[d.material].name, if d.group == 3 { "world" } else { scene.model_names.get(d.source as usize).map(String::as_str).unwrap_or("model") })
                    })
                    .collect();
                println!("({x:.0}, {y:.0}): {}", s.join(" | "));
            }
        }
        rows.push(row);
    }
    // North (+y) up, east (+x) right; '.' nothing under the point.
    println!("floor map ({step} units a character, x {} .. {}, y {} .. {} from the bottom row up):", cx - r, cx + r, cy - r, cy + r);
    for row in rows.iter().rev() {
        println!("  {row}");
    }
    let mut l: Vec<_> = legend.iter().collect();
    l.sort_by_key(|e| *e.1);
    for (name, k) in l {
        let ch = char::from(if *k < 26 { b'a' + *k as u8 } else if *k < 52 { b'A' + (*k - 26) as u8 } else { b'#' });
        println!("  {ch} = {name}");
    }
    let mut v: Vec<_> = top.into_iter().collect();
    v.sort_by(|p, q| q.1.cmp(&p.1));
    println!("topmost surface per grid point ({} points):", (n + 1) * (n + 1));
    for (name, c) in &v {
        println!("  {c:5} {name}");
    }
    println!("every surface hit (points):");
    let mut v: Vec<_> = any.into_iter().collect();
    v.sort_by(|p, q| q.1.cmp(&p.1));
    for (name, c) in &v {
        let Some(&di) = seen_draw.get(name) else { continue };
        let d = &scene.draws[di];
        let m = &scene.materials[d.material];
        let tex: Vec<String> = m.textures.iter().map(|(k, t)| format!("{k:08x}={t}")).collect();
        println!(
            "  {c:5} {name}: {} list {} sort {:016x} g {} fwd {} gb {} pre {}; {}",
            m.techset,
            d.list,
            m.sort_key,
            d.group,
            m.forward.is_some(),
            m.technique.is_some(),
            m.prepass.is_some(),
            tex.join(" ")
        );
    }
}
