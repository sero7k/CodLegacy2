//! bo2mp: a T6 map's water materials (technique set names holding
//! "water"): constants, textures with their image sizes, and the world
//! area they cover: `t6_water_probe <zone.ff> [out dir]`. With an out dir,
//! also each texture's top level as a PNG with its channels' min, mean and
//! max, and each water surface's vertices (position, texcoord).
use std::collections::HashMap;

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((h >> 10) & 0x1f);
    let m = f32::from(h & 0x3ff);
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => s * f32::INFINITY,
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

fn rgba8(iwi: &ipak_t6::IwiImage<'_>) -> Option<Vec<u8>> {
    use ipak_t6::IwiFormat as F;
    let (w, h) = (iwi.width as usize, iwi.height as usize);
    let top = &iwi.level(0)[..iwi.level_bytes(0)];
    let (block, decode): (usize, fn(&[u8], &mut [u8], usize)) = match iwi.format {
        F::Dxt1 => (8, bcdec_rs::bc1),
        F::Dxt3 => (16, bcdec_rs::bc2),
        F::Dxt5 => (16, bcdec_rs::bc3),
        F::Rgba8 => return Some(top.to_vec()),
        F::Luminance => return Some(top.iter().flat_map(|&l| [l, l, l, 255]).collect()),
        F::LuminanceAlpha => return Some(top.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()),
        F::Alpha => return Some(top.iter().flat_map(|&a| [255, 255, 255, a]).collect()),
        _ => return None,
    };
    let mut out = vec![0u8; w * h * 4];
    let mut tile = [0u8; 64];
    let bw = w.div_ceil(4);
    for by in 0..h.div_ceil(4) {
        for bx in 0..bw {
            let at = (by * bw + bx) * block;
            decode(top.get(at..at + block)?, &mut tile, 16);
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let d = (py * w + px) * 4;
                        out[d..d + 4].copy_from_slice(&tile[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4]);
                    }
                }
            }
        }
    }
    Some(out)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::Path::new(&a[1]);
    let out = a.get(2).map(std::path::PathBuf::from);
    let c = asset_t6::capture_zone(zone).unwrap();
    let packs = out.as_ref().map(|_| asset_t6::PackSet::open_dir(zone.parent().unwrap()).unwrap());
    let w = c.world.as_ref().unwrap();
    // Per material: triangles and the xy box of its surfaces.
    let mut cover: HashMap<usize, (u64, [f32; 4])> = HashMap::new();
    for s in &w.surfaces {
        let Some(m) = s.material else { continue };
        let e = cover.entry(m.index).or_insert((0, [f32::MAX, f32::MAX, f32::MIN, f32::MIN]));
        e.0 += u64::from(s.tri_count);
        let b = s.bounds;
        e.1 = [e.1[0].min(b[0][0]), e.1[1].min(b[0][1]), e.1[2].max(b[1][0]), e.1[3].max(b[1][1])];
    }
    for (i, m) in c.materials.iter().enumerate() {
        let ts = m.technique_set.and_then(|k| c.technique_sets.get(k.index)).map_or("", |t| t.name.as_str());
        if !ts.contains("water") {
            continue;
        }
        println!("{} [{ts}] sort={} world={:?}", m.name, m.sort_key, cover.get(&i));
        for (h, n, v) in &m.constants {
            println!("    const {h:08x} {n} {v:?}");
        }
        for t in &m.textures {
            let img = t.image.map(|k| &c.images[k.index]);
            println!(
                "    tex {:08x} sem{} samp{:#x} {}",
                t.name_hash,
                t.semantic,
                t.sampler_state,
                img.map_or("-".into(), |i| format!("{} {}x{} mips{} type{}", i.name, i.width, i.height, i.level_count, i.map_type))
            );
            let (Some(dir), Some(packs), Some(img)) = (&out, &packs, img) else { continue };
            let Some(asset_t6::ImageSource::Pack(p, e)) = packs.locate(img) else {
                println!("        not in a pack");
                continue;
            };
            let bytes = packs.packs[p].read(e).unwrap();
            let iwi = ipak_t6::parse_iwi(&bytes).unwrap();
            print!("        iwi {:?} {}x{} levels{} faces{}", iwi.format, iwi.width, iwi.height, iwi.levels, iwi.faces);
            let Some(px) = rgba8(&iwi) else {
                println!();
                continue;
            };
            let n = (iwi.width * iwi.height) as usize;
            for ch in 0..4 {
                let v = px.iter().skip(ch).step_by(4).take(n);
                let (lo, hi, sum) = v.fold((255u8, 0u8, 0u64), |(lo, hi, s), &x| (lo.min(x), hi.max(x), s + u64::from(x)));
                print!(" {}[{lo} {:.0} {hi}]", ['r', 'g', 'b', 'a'][ch], sum as f64 / n as f64);
            }
            println!();
            if iwi.faces == 1 {
                let name: String = img.name.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' { ch } else { '-' }).collect();
                let f = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), iwi.width, iwi.height);
                enc.set_color(png::ColorType::Rgba);
                enc.write_header().unwrap().write_image_data(&px[..n * 4]).unwrap();
            }
        }
        if out.is_some() {
            for s in w.surfaces.iter().filter(|s| s.material.is_some_and(|k| k.index == i)).take(4) {
                println!(
                    "    surface vd0 {} first {} verts {} tris {}",
                    s.vertex_data_offset0, s.first_vertex, s.vertex_count, s.tri_count
                );
                for v in 0..usize::from(s.vertex_count) {
                    let at = s.vertex_data_offset0 as usize + v * 36;
                    let Some(b) = w.vd0.get(at..at + 36) else { break };
                    let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                    let h = |o: usize| half(u16::from_le_bytes(b[o..o + 2].try_into().unwrap()));
                    println!("        pos [{:.0} {:.0} {:.0}] uv [{:.4} {:.4}]", f(0), f(4), f(8), h(20), h(22));
                }
            }
        }
    }
}
