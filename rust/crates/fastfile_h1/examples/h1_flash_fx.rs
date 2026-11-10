//! The first-person muzzle flash effect of every gun in a set of zones,
//! element by element: its type, its first visual sample's colour (RGBA,
//! emissive scale), and for a sprite its material's technique set and
//! pictures (name, DXGI format). Materials and effects a zone only refers to
//! are looked up in the other zones given.
//!
//! `cargo run -p fastfile_h1 --example h1_flash_fx -- [--only <substr>] <zone.ff>...`

use std::collections::HashMap;

use fastfile_h1::layout::{FxElemVisStateSample as V, WeaponDef as D};
use fastfile_h1::walk::{Capture, FxRec, walk_zone};

struct Z {
    path: std::path::PathBuf,
    image: Vec<u8>,
    cap: Capture,
}

/// An image's mean colour (gamma-space RGB, alpha, and RGB weighted by
/// alpha), decoded from wherever its pixels are.
fn image_mean(zones: &[Z], name: &str) -> Option<[f32; 7]> {
    // (The biggest of the name: a zone's 1 x 1 stand-in loses to the real one.)
    let (z, rec) = zones
        .iter()
        .flat_map(|z| {
            z.cap
                .images
                .iter()
                .filter(|r| r.name.trim_start_matches(',') == name && !fastfile_h1::shared::is_reference(r))
                .map(move |r| (z, r))
        })
        .max_by_key(|(_, r)| u32::from(r.width) * u32::from(r.height) + u32::from(r.streamed))?;
    let retail = std::env::var_os("H1_IMAGEFILES").map(std::path::PathBuf::from);
    let mut paks = fastfile_h1::image::ImagePaks::for_zone(&z.path, retail.as_deref());
    let px = fastfile_h1::image::image_pixels(rec, &z.image, &mut paks, 256).map_err(|e| eprintln!("{name}: {e}")).ok()?;
    eprintln!(
        "  {name}: {}x{} fmt {} levels {} streamed {} (zone {}x{}) first bytes {:?}",
        px.width, px.height, px.dxgi_format, px.levels, rec.streamed, rec.width, rec.height,
        &px.data[..px.data.len().min(16)]
    );
    let rgba: Vec<[f32; 4]> = if matches!(px.dxgi_format, 95 | 96) {
        let (w, h) = (px.width as usize, px.height as usize);
        let mut out = vec![[0f32; 4]; w * h];
        let mut block = [0f32; 48];
        for by in 0..h / 4 {
            for bx in 0..w / 4 {
                let b = &px.data[(by * (w / 4) + bx) * 16..][..16];
                bcdec_rs::bc6h_float(b, &mut block, 12, px.dxgi_format == 96);
                for y in 0..4 {
                    for x in 0..4 {
                        let t = &block[y * 12 + x * 3..][..3];
                        out[(by * 4 + y) * w + bx * 4 + x] = [t[0], t[1], t[2], 1.0];
                    }
                }
            }
        }
        out
    } else {
        let (_, _, b) = fastfile_h1::image::rgba8_level0(&px).map_err(|e| eprintln!("{name}: {e}")).ok()?;
        b.chunks_exact(4)
            .map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0])
            .collect()
    };
    let n = rgba.len().max(1) as f32;
    let mut m = [0f32; 7];
    for t in &rgba {
        for k in 0..4 {
            m[k] += t[k] / n;
        }
        for k in 0..3 {
            m[4 + k] += t[k] * t[3] / n;
        }
    }
    Some(m)
}

fn main() {
    let mut only = None;
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--only" {
            only = args.next();
        } else {
            paths.push(a);
        }
    }
    let mut zones: Vec<Z> = Vec::new();
    for p in &paths {
        match fastfile_h1::Zone::open(std::path::Path::new(p)) {
            Ok(z) => {
                let cap = walk_zone(&z).0;
                zones.push(Z { path: p.into(), image: z.image, cap });
            }
            Err(e) => eprintln!("{p}: {e}"),
        }
    }
    let mut formats: HashMap<String, u32> = HashMap::new();
    for z in &zones {
        for im in &z.cap.images {
            if im.format != 0 {
                formats.insert(im.name.trim_start_matches(',').to_owned(), im.format);
            }
        }
    }
    if std::env::var_os("IMAGE_CENSUS").is_some() {
        let mut census: std::collections::BTreeMap<(u32, u8, u8, u8), (usize, Vec<String>)> = Default::default();
        for z in &zones {
            for im in &z.cap.images {
                if im.format == 0 {
                    continue;
                }
                let e = census.entry((im.format, im.semantic, im.category, im.flags)).or_default();
                e.0 += 1;
                if e.1.len() < 4 {
                    e.1.push(im.name.clone());
                }
            }
        }
        for ((f, sem, cat, fl), (n, names)) in census {
            println!("fmt {f} sem {sem} cat {cat} flags {fl:#x}: {n} e.g. {names:?}");
        }
        return;
    }
    if let Some(n) = std::env::var_os("FX_NAME") {
        let n = n.to_string_lossy().into_owned();
        println!("{n}");
        dump(&n, &zones, &formats, 1);
        return;
    }
    for z in &zones {
        for w in &z.cap.weapons {
            if only.as_deref().is_some_and(|o| !w.name.contains(o)) {
                continue;
            }
            if std::env::var_os("FLASH_ALL").is_some() {
                for (off, id) in &w.fx {
                    println!("{} fx at {off:#x}: {}", w.name, z.cap.name_of(*id));
                }
                for (off, name) in [
                    (D::viewFlashEffect, "viewFlashEffect"),
                    (D::viewBodyFlashEffect, "viewBodyFlashEffect"),
                    (D::worldFlashEffect, "worldFlashEffect"),
                    (D::viewFlashADSEffect, "viewFlashADSEffect"),
                    (D::viewBodyFlashADSEffect, "viewBodyFlashADSEffect"),
                ] {
                    println!("  ({name} is {off:#x})");
                }
            }
            for (off, id) in &w.fx {
                if *off != D::viewFlashEffect && std::env::var_os("FLASH_ALL").is_none() {
                    continue;
                }
                let name = z.cap.name_of(*id).trim_start_matches(',').to_owned();
                println!("{} viewFlashEffect {}", w.name, name);
                dump(&name, &zones, &formats, 1);
            }
        }
    }
}

fn find_fx<'a>(zones: &'a [Z], name: &str) -> Option<(&'a Z, &'a FxRec)> {
    zones.iter().find_map(|z| {
        z.cap
            .fx
            .iter()
            .find(|f| !f.name.starts_with(',') && f.name == name)
            .map(|f| (z, f))
    })
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4)
        .map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn dump(name: &str, zones: &[Z], formats: &HashMap<String, u32>, depth: usize) {
    let pad = "  ".repeat(depth);
    let Some((z, f)) = find_fx(zones, name) else {
        println!("{pad}(effect {name} not in these zones)");
        return;
    };
    for (i, el) in f.elems.iter().enumerate() {
        let vis = el.vis_samples.map(|s| s.get(&z.image).to_vec()).unwrap_or_default();
        let c: Vec<f32> = (0..4).map(|k| f32_at(&vis, V::base__color + 4 * k)).collect();
        let e: Vec<f32> = (0..3).map(|k| f32_at(&vis, V::base__emissiveScale + 4 * k)).collect();
        println!(
            "{pad}elem {i} type {} colour [{:.3} {:.3} {:.3} {:.3}] emissive [{:.4} {:.4} {:.4}] size {:.1} scale {:.2} samples {}",
            el.elem_type,
            c[0],
            c[1],
            c[2],
            c[3],
            e[0],
            e[1],
            e[2],
            f32_at(&vis, V::base__size),
            f32_at(&vis, V::base__scale),
            vis.len() / V::SIZE
        );
        if std::env::var_os("FLASH_V").is_some() {
            use fastfile_h1::layout::FxElemDef as E;
            let r = el.record.get(&z.image);
            let i32_at = |at: usize| r.get(at..at + 4).map_or(0, |s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]));
            println!(
                "{pad}  flags {:#x} flags2 {:#x} life {}+{} delay {}+{} origin x {:.1}+{:.1} lightingFrac {} atlas idx {} cols {} rows {} vis_intervals {}",
                i32_at(E::flags),
                i32_at(E::flags2),
                i32_at(E::lifeSpanMsec),
                i32_at(E::lifeSpanMsec + 4),
                i32_at(E::spawnDelayMsec),
                i32_at(E::spawnDelayMsec + 4),
                f32_at(r, E::spawnOrigin),
                f32_at(r, E::spawnOrigin + 4),
                r[E::lightingFrac],
                r[E::atlas + 1],
                r[E::atlas + 4],
                r[E::atlas + 5],
                r[E::visStateIntervalCount],
            );
            for (k, smp) in vis.chunks_exact(V::SIZE).enumerate() {
                println!(
                    "{pad}  sample {k}: base c [{:.3} {:.3} {:.3} {:.3}] e {:.4} size {:.2}/{:.2} scale {:.2} | amp c [{:.3} {:.3} {:.3} {:.3}] e {:.4} size {:.2}/{:.2}",
                    f32_at(smp, V::base__color),
                    f32_at(smp, V::base__color + 4),
                    f32_at(smp, V::base__color + 8),
                    f32_at(smp, V::base__color + 12),
                    f32_at(smp, V::base__emissiveScale),
                    f32_at(smp, V::base__size),
                    f32_at(smp, V::base__size + 4),
                    f32_at(smp, V::base__scale),
                    f32_at(smp, V::amplitude__color),
                    f32_at(smp, V::amplitude__color + 4),
                    f32_at(smp, V::amplitude__color + 8),
                    f32_at(smp, V::amplitude__color + 12),
                    f32_at(smp, V::amplitude__emissiveScale),
                    f32_at(smp, V::amplitude__size),
                    f32_at(smp, V::amplitude__size + 4),
                );
            }
        }
        for m in &el.materials {
            let mname = z.cap.name_of(*m).trim_start_matches(',').to_owned();
            let found = zones.iter().find_map(|o| {
                o.cap
                    .materials
                    .iter()
                    .position(|r| !r.name.starts_with(',') && r.name == mname)
                    .map(|i| (&o.cap, i))
            });
            match found {
                Some((c, i)) => print_material(c, i, zones, formats, &format!("{pad}  ")),
                None => println!("{pad}  material {mname} (not in these zones)"),
            }
        }
        for e in &el.effects {
            if depth < 5 {
                println!("{pad}  child {e}");
                dump(e.trim_start_matches(','), zones, formats, depth + 2);
            }
        }
    }
}

fn print_material(cap: &Capture, index: usize, zones: &[Z], formats: &HashMap<String, u32>, pad: &str) {
    let Some(m) = cap.materials.get(index) else {
        return;
    };
    let tech = m.techset.map(|t| cap.name_of(t).to_owned()).unwrap_or_default();
    println!("{pad}material {} tech {}", m.name, tech);
    for t in &m.textures {
        let img = t
            .image
            .map(|i| cap.name_of(i).trim_start_matches(',').to_owned())
            .unwrap_or_default();
        println!(
            "{pad}  tex {:08x} sem {} {} fmt {:?}",
            t.name_hash,
            t.semantic,
            img,
            formats.get(&img)
        );
        if t.semantic == 2
            && let Some(m) = image_mean(zones, &img)
        {
            println!(
                "{pad}    mean rgb [{:.3} {:.3} {:.3}] a {:.3} rgb*a [{:.3} {:.3} {:.3}]",
                m[0], m[1], m[2], m[3], m[4], m[5], m[6]
            );
        }
    }
}
