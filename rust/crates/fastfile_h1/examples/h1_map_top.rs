//! A top-down check of an H1 map zone: `h1_map_top <zone.ff> <out.png>`.
//! Draws the lit world surfaces (one colour a surface, highest z wins, the
//! second level above `alternate_level_cut` left out), the spawn points as
//! white dots over the view the 2nd..98th percentile of the spawns frame,
//! and prints the numbers a layout check wants: bounds of the drawn world,
//! of the spawns and of the collision, how many spawns stand over a drawn
//! surface, and how much of the drawn area the spawns' frame covers.
use std::path::PathBuf;

const W: usize = 960;
const H: usize = 540;

fn main() {
    let mut a = std::env::args_os().skip(1);
    let (Some(zone_path), Some(out)) = (a.next(), a.next()) else {
        eprintln!("usage: h1_map_top <zone.ff> <out.png>");
        std::process::exit(2);
    };
    let zone_path = PathBuf::from(zone_path);
    let zone = match fastfile_h1::Zone::open(&zone_path) {
        Ok(z) => z,
        Err(e) => {
            println!("{}: open: {e}", zone_path.display());
            return;
        }
    };
    let (map, cap) = fastfile_h1::map::IwMap::load(&zone);
    let s = fastfile_h1::map::summary(&map, &cap);
    println!(
        "{}: surfaces {} verts {} static models {} brush models {} brushes {} coll tris {} spawns {}",
        zone_path.file_name().unwrap().to_string_lossy(),
        s.surfaces, s.vertices, s.static_models, s.brush_models, s.brushes, s.collision_tris, s.spawns
    );
    for n in &map.notes {
        println!("  note: {n}");
    }
    let Some(world) = map.world.as_ref() else {
        println!("  NO WORLD");
        return;
    };
    let cut = fastfile_h1::map::alternate_level_cut(&map.spawns);
    println!("  second level cut {cut:?}; world bounds {:?}..{:?}", world.mins, world.maxs);
    if let Some(c) = map.clip.as_ref() {
        println!("  clip: {} brushes, {} tris, {} cmodels", c.brushes.len(), c.tri_indices.len() / 3, c.cmodels.len());
        if let Some(m) = c.cmodels.first() {
            println!("  clip cmodel0 {:?}..{:?}", m.mins, m.maxs);
        }
    } else {
        println!("  NO CLIP");
    }
    // Spawn frame.
    let (mut mn, mut mx) = ([f32::MAX; 2], [f32::MIN; 2]);
    let (mut smn, mut smx) = ([f32::MAX; 3], [f32::MIN; 3]);
    for sp in &map.spawns {
        for k in 0..3 {
            smn[k] = smn[k].min(sp.origin[k]);
            smx[k] = smx[k].max(sp.origin[k]);
        }
    }
    for k in 0..2 {
        let mut v: Vec<f32> = map.spawns.iter().map(|s| s.origin[k]).collect();
        v.sort_by(f32::total_cmp);
        if v.is_empty() {
            mn[k] = world.mins[k];
            mx[k] = world.maxs[k];
        } else {
            let c = v.len() / 50;
            mn[k] = v[c];
            mx[k] = v[v.len() - 1 - c];
        }
    }
    println!("  spawn bounds {smn:?}..{smx:?}");
    let pad = 600.0;
    let (a0, b0) = ([mn[0] - pad, mn[1] - pad], [mx[0] + pad, mx[1] + pad]);
    let (cx, cy) = ((a0[0] + b0[0]) / 2.0, (a0[1] + b0[1]) / 2.0);
    let (hw, hh) = ((b0[0] - a0[0]) / 2.0, (b0[1] - a0[1]) / 2.0);
    // World -> pixel: pixel size `ps` units.
    let ps = (2.0 * hw / W as f32).max(2.0 * hh / H as f32);
    let (x0, y1) = (cx - ps * W as f32 / 2.0, cy + ps * H as f32 / 2.0);
    let mut zb = vec![f32::MIN; W * H];
    let mut col = vec![[40u8, 40, 48]; W * H];
    let in_range = |i: u32| world.ranges.iter().any(|&(a, b)| i >= a.min(b) && i < b);
    let mut drawn_tris = 0usize;
    for (si, s) in world.surfaces.iter().enumerate() {
        if !in_range(si as u32) || s.material.is_none() {
            continue;
        }
        if cut.is_some_and(|c| s.mins[2] > c) {
            continue;
        }
        // Sky domes and vista sheets (wider than 16000 units) and what hangs
        // more than 1500 above the highest spawn would hide the ground.
        if (s.maxs[0] - s.mins[0]).max(s.maxs[1] - s.mins[1]) > 16000.0 || s.mins[2] > smx[2] + 1500.0 {
            continue;
        }
        let h = (si as u32).wrapping_mul(2654435761);
        let base = [(h >> 8) as u8 | 64, (h >> 16) as u8 | 64, (h >> 24) as u8 | 64];
        for t in 0..s.tri_count as usize {
            let mut p = [[0f32; 3]; 3];
            let mut ok = true;
            for (k, pk) in p.iter_mut().enumerate() {
                let Some(&ix) = world.indices.get(s.base_index as usize + 3 * t + k) else {
                    ok = false;
                    break;
                };
                let Some(v) = world.vertices.get(s.first_vertex as usize + ix as usize) else {
                    ok = false;
                    break;
                };
                *pk = v.xyz;
            }
            if !ok {
                continue;
            }
            drawn_tris += 1;
            let px = |q: [f32; 3]| ((q[0] - x0) / ps, (y1 - q[1]) / ps);
            let (a, b, c) = (px(p[0]), px(p[1]), px(p[2]));
            let (minx, maxx) = (a.0.min(b.0).min(c.0).floor().max(0.0), a.0.max(b.0).max(c.0).ceil().min(W as f32));
            let (miny, maxy) = (a.1.min(b.1).min(c.1).floor().max(0.0), a.1.max(b.1).max(c.1).ceil().min(H as f32));
            if minx >= maxx || miny >= maxy {
                continue;
            }
            let den = (b.1 - c.1) * (a.0 - c.0) + (c.0 - b.0) * (a.1 - c.1);
            if den.abs() < 1e-9 {
                continue;
            }
            for y in miny as usize..maxy as usize {
                for x in minx as usize..maxx as usize {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let l1 = ((b.1 - c.1) * (fx - c.0) + (c.0 - b.0) * (fy - c.1)) / den;
                    let l2 = ((c.1 - a.1) * (fx - c.0) + (a.0 - c.0) * (fy - c.1)) / den;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                        continue;
                    }
                    let z = l1 * p[0][2] + l2 * p[1][2] + l3 * p[2][2];
                    if z > zb[y * W + x] {
                        zb[y * W + x] = z;
                        col[y * W + x] = base;
                    }
                }
            }
        }
    }
    let mut over = 0usize;
    let mut inside = 0usize;
    for sp in &map.spawns {
        let (x, y) = ((sp.origin[0] - x0) / ps, (y1 - sp.origin[1]) / ps);
        if x >= 0.0 && y >= 0.0 && (x as usize) < W && (y as usize) < H {
            inside += 1;
            if zb[y as usize * W + x as usize] > f32::MIN {
                over += 1;
            }
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let (px, py) = (x as i32 + dx, y as i32 + dy);
                    if px >= 0 && py >= 0 && (px as usize) < W && (py as usize) < H {
                        col[py as usize * W + px as usize] = [255, 255, 255];
                    }
                }
            }
        }
    }
    let covered = zb.iter().filter(|&&z| z > f32::MIN).count();
    println!(
        "  drawn tris {drawn_tris}; spawns in frame {inside}/{}, over a drawn surface {over}; frame covered {:.0}%",
        map.spawns.len(),
        100.0 * covered as f32 / (W * H) as f32
    );
    let f = std::fs::File::create(&out).expect("png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), W as u32, H as u32);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    let flat: Vec<u8> = col.iter().flatten().copied().collect();
    e.write_header().unwrap().write_image_data(&flat).unwrap();
}
