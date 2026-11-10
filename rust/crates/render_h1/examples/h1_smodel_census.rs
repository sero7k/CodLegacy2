//! Static models by xmodel: placements, and whether the first LOD has
//! drawable surfaces (vertices and triangles in the zone) and materials.
//!
//! `cargo run -p render_h1 --release --example h1_smodel_census -- <map.ff> [name filter]`
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let filter = a.get(1).cloned().unwrap_or_default();
    let z = fastfile_h1::Zone::open(std::path::Path::new(&a[0])).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    let map = fastfile_h1::map::read_map(&cap, &z.image, &z.assets.script_strings);
    let w = map.world.expect("world");
    println!(
        "light grid colours {}, light grid {}, grid-lit models {}, with ground lighting {}",
        w.light_grid_colors.len(),
        w.light_grid
            .as_ref()
            .map_or("none".to_string(), |g| format!(
                "rows {} entries {}",
                g.row_start.len(),
                g.entry_colors.len()
            )),
        w.static_models
            .iter()
            .filter(|m| m.grid_color.is_some())
            .count(),
        w.static_models
            .iter()
            .filter(|m| m.flags & 0x20 != 0)
            .count(),
    );
    for n in &map.notes {
        println!("note: {n}");
    }
    let in_range = |i: usize| w.ranges.iter().any(|&(b, e)| (b as usize..e as usize).contains(&i));
    let outside: Vec<usize> = (0..w.surfaces.len()).filter(|&i| !in_range(i)).collect();
    println!(
        "world surfaces {}, ranges {:?}, outside every range {} (first {:?})",
        w.surfaces.len(),
        w.ranges,
        outside.len(),
        outside.iter().take(5).collect::<Vec<_>>()
    );
    // Flags of placements by whether their model's materials are `ml/`.
    let mut flags_ml: BTreeMap<u16, usize> = BTreeMap::new();
    let mut flags_other: BTreeMap<u16, usize> = BTreeMap::new();
    for sm in &w.static_models {
        let Some(m) = sm.model else { continue };
        let xm = &cap.xmodels[m.index as usize];
        let ml = xm
            .materials
            .iter()
            .flatten()
            .any(|id| cap.name_of(*id).trim_start_matches(',').starts_with("ml/"));
        *if ml { &mut flags_ml } else { &mut flags_other }
            .entry(sm.flags)
            .or_default() += 1;
    }
    println!("flags of ml/ placements {flags_ml:x?}\nflags of other placements {flags_other:x?}");
    let mut lm_models: BTreeMap<String, (usize, [f32; 3])> = BTreeMap::new();
    for sm in w.static_models.iter().filter(|m| m.flags & 0x100 != 0) {
        let Some(m) = sm.model else { continue };
        let e = lm_models
            .entry(cap.xmodels[m.index as usize].name.clone())
            .or_insert((0, sm.origin));
        e.0 += 1;
    }
    println!("model-lightmapped (flag 0x100) placements by model: {lm_models:?}");
    for sm in w.static_models.iter().filter(|m| m.flags & 0x100 != 0).take(3) {
        let Some(m) = sm.model else { continue };
        let xm = &cap.xmodels[m.index as usize];
        println!("  {} lightmap {:?}", xm.name, sm.model_lightmap);
        let ms = xm.lods.first().and_then(|l| l.surfs).and_then(|s| cap.model_surfs.get(s.index as usize));
        if let Some(s) = ms.and_then(|ms| ms.surfs.first())
            && let Some(u) = s.lmap_unwrap
        {
            let b = u.get(&z.image);
            for c in b.chunks_exact(4).take(4) {
                let h = |x: u16| {
                    let s = if x & 0x8000 != 0 { -1.0f32 } else { 1.0 };
                    let e = i32::from((x >> 10) & 31);
                    let m = f32::from(x & 1023);
                    match e {
                        0 => s * m * 2f32.powi(-24),
                        31 => s * f32::INFINITY,
                        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
                    }
                };
                let (a, bb) = (u16::from_le_bytes([c[0], c[1]]), u16::from_le_bytes([c[2], c[3]]));
                println!("    unwrap half ({:.4}, {:.4}) unorm ({:.4}, {:.4})", h(a), h(bb), f32::from(a) / 65535.0, f32::from(bb) / 65535.0);
            }
        }
    }
    let mut by: BTreeMap<u32, usize> = BTreeMap::new();
    for sm in &w.static_models {
        if let Some(m) = sm.model {
            *by.entry(m.index).or_default() += 1;
        }
    }
    let (mut ok, mut bad) = (0usize, 0usize);
    for (&mi, &n) in &by {
        let xm = &cap.xmodels[mi as usize];
        let name = xm.name.clone();
        let lod = xm.lods.first();
        let ms = lod
            .and_then(|l| l.surfs)
            .and_then(|s| cap.model_surfs.get(s.index as usize));
        let (surfs, drawable) = ms.map_or((0, 0), |ms| {
            (
                ms.surfs.len(),
                ms.surfs
                    .iter()
                    .filter(|s| s.verts.is_some() && s.tris.is_some())
                    .count(),
            )
        });
        let good = drawable > 0;
        if good {
            ok += n
        } else {
            bad += n
        }
        if name.contains(filter.as_str()) && (!good || !filter.is_empty()) {
            println!(
                "{name}: {n} placed, lods {}, surfs {surfs}, drawable {drawable}, materials {}",
                xm.lods.len(),
                xm.materials.len()
            );
        }
    }
    if !filter.is_empty() {
        for sm in w.static_models.iter().take_while(|_| true) {
            let Some(m) = sm.model else { continue };
            let xm = &cap.xmodels[m.index as usize];
            if xm.name.contains(filter.as_str()) {
                println!(
                    "  {} at {:?} scale {} flags {:#x} axis {:?}",
                    xm.name, sm.origin, sm.scale, sm.flags, sm.axis
                );
                // Local vertex bounds of LOD 0 and the placed bounds with
                // the axis rows taken as rows (forward/left/up) or columns.
                let ms = xm
                    .lods
                    .first()
                    .and_then(|l| l.surfs)
                    .and_then(|s| cap.model_surfs.get(s.index as usize));
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                let (mut wr_lo, mut wr_hi, mut wc_lo, mut wc_hi) =
                    ([f32::MAX; 3], [f32::MIN; 3], [f32::MAX; 3], [f32::MIN; 3]);
                for s in ms.map_or(&[][..], |m| &m.surfs[..]) {
                    let Some(v) = s.verts else { continue };
                    for c in v.get(&z.image).chunks_exact(32) {
                        let p: [f32; 3] = std::array::from_fn(|i| {
                            f32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().unwrap())
                        });
                        for i in 0..3 {
                            lo[i] = lo[i].min(p[i]);
                            hi[i] = hi[i].max(p[i]);
                            let r = sm.origin[i]
                                + sm.scale * (0..3).map(|k| sm.axis[k][i] * p[k]).sum::<f32>();
                            let cpos = sm.origin[i]
                                + sm.scale * (0..3).map(|k| sm.axis[i][k] * p[k]).sum::<f32>();
                            wr_lo[i] = wr_lo[i].min(r);
                            wr_hi[i] = wr_hi[i].max(r);
                            wc_lo[i] = wc_lo[i].min(cpos);
                            wc_hi[i] = wc_hi[i].max(cpos);
                        }
                    }
                }
                println!(
                    "    local {lo:?}..{hi:?}\n    as now {wr_lo:?}..{wr_hi:?}\n    transposed {wc_lo:?}..{wc_hi:?}"
                );
            }
        }
    }
    println!("placements with drawable surfaces {ok}, without {bad}");
}
