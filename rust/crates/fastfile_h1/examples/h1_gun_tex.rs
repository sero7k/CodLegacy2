//! A gun's view model materials, texture by texture: the slot (name hash and
//! its known name), the picture, its format, size, whether it streams, and
//! its decoded mean / maximum red, green, blue and alpha; then the
//! material's constants. For finding out which picture is the colour and
//! how it is stored.
//!
//! `cargo run -p fastfile_h1 --example h1_gun_tex -- <retail zone dir> <gun substr> <zone.ff>...`

use std::collections::BTreeSet;
use std::path::Path;

/// The engine's name hash: lower-cased bytes, `h = c ^ 33 * h`.
fn name_hash(s: &str) -> u32 {
    s.bytes()
        .fold(0u32, |h, c| u32::from(c | 0x20) ^ h.wrapping_mul(33))
}

fn slot_name(hash: u32) -> String {
    const BASES: [&str; 24] = [
        "color",
        "normal",
        "specular",
        "emissive",
        "detail",
        "detailNormal",
        "gloss",
        "occlusion",
        "mask",
        "camo",
        "camoMask",
        "camoColor",
        "camoNormal",
        "camoSpecular",
        "camoGloss",
        "cavity",
        "reveal",
        "alphaReveal",
        "thickness",
        "height",
        "envBrdf",
        "specularGloss",
        "translucency",
        "displacement",
    ];
    for b in BASES {
        for suffix in ["Map", "", "Lut", "LUT"] {
            for n in ["", "0", "1", "2"] {
                let name = format!("{b}{suffix}{n}");
                if name_hash(&name) == hash {
                    return name;
                }
            }
        }
    }
    "?".to_owned()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(retail), Some(gun)) = (a.first(), a.get(1)) else {
        eprintln!("usage: h1_gun_tex <retail zone dir> <gun substr> <zone.ff>...");
        std::process::exit(2);
    };
    let retail = Path::new(retail);
    for path in &a[2..] {
        let zone = fastfile_h1::Zone::open(Path::new(path)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        let mut paks = fastfile_h1::image::ImagePaks::for_zone(Path::new(path), Some(retail));
        for w in cap.weapons.iter().filter(|w| w.name.contains(gun.as_str())) {
            let mut seen = BTreeSet::new();
            for id in w.gun_models.iter().flatten() {
                let Some(m) = cap.xmodels.get(id.index as usize) else {
                    continue;
                };
                println!("{} view model {}", w.name, m.name);
                // The vertex colour (bytes 16..20 of each packed vertex): the
                // game multiplies it into the colour map before squaring.
                let parts: Vec<_> = if m.composite.is_empty() {
                    vec![m]
                } else {
                    m.composite
                        .iter()
                        .flatten()
                        .filter_map(|p| cap.xmodels.get(p.index as usize))
                        .collect()
                };
                for part in parts {
                    let Some(surfs) = part
                        .lods
                        .first()
                        .and_then(|l| l.surfs)
                        .and_then(|s| cap.model_surfs.get(s.index as usize))
                    else {
                        continue;
                    };
                    for (k, sf) in surfs.surfs.iter().enumerate() {
                        let bytes = sf.verts.map_or(&[][..], |sp| sp.get(&zone.image));
                        let (mut sum, mut max, mut n) = ([0u64; 4], [0u8; 4], 0u64);
                        for v in bytes.chunks_exact(32) {
                            for c in 0..4 {
                                sum[c] += u64::from(v[16 + c]);
                                max[c] = max[c].max(v[16 + c]);
                            }
                            n += 1;
                        }
                        let n = n.max(1) as f64;
                        println!(
                            "  vertex colour {} surf {k}: {} verts, bytes 16..20 mean {:.0} {:.0} {:.0} {:.0} max {:?}",
                            part.name,
                            sf.vert_count,
                            sum[0] as f64 / n,
                            sum[1] as f64 / n,
                            sum[2] as f64 / n,
                            sum[3] as f64 / n,
                            max
                        );
                    }
                }
                let mut mats: Vec<_> = m.materials.iter().flatten().copied().collect();
                for p in m.composite.iter().flatten() {
                    if let Some(pm) = cap.xmodels.get(p.index as usize) {
                        mats.extend(pm.materials.iter().flatten().copied());
                    }
                }
                for mat in mats {
                    if !seen.insert(mat.index) {
                        continue;
                    }
                    let Some(mr) = cap.materials.get(mat.index as usize) else {
                        continue;
                    };
                    let tech = mr.techset.map(|t| cap.name_of(t)).unwrap_or("-");
                    println!("  material {} tech {}", cap.name_of(mat), tech);
                    for t in &mr.textures {
                        let Some(i) = t.image else { continue };
                        let rec = &cap.images[i.index as usize];
                        // H1_BLOCKS=1: the first block-compressed blocks of each picture, raw.
                        if std::env::var_os("H1_BLOCKS").is_some()
                            && !rec.streamed
                            && let Ok(px) =
                                fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, 4096)
                            && let Some((unit, true)) = fastfile_h1::image::dxgi_unit(rec.format)
                        {
                            let mid = px.data.len() / 2 / unit * unit;
                            let hex =
                                |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
                            println!(
                                "      blocks first {} | middle {}",
                                hex(&px.data[..unit * 2]),
                                hex(&px.data[mid..mid + unit * 2])
                            );
                        }
                        let stats =
                            fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, 4096)
                                .and_then(|px| {
                                    fastfile_h1::image::rgba8_level0(&px).map(|r| (px.levels, r))
                                });
                        let tail = match stats {
                            Ok((levels, (_, _, rgba))) => {
                                let n = (rgba.len() / 4).max(1) as f64;
                                let mut sum = [0f64; 4];
                                let mut max = [0u8; 4];
                                for p in rgba.chunks_exact(4) {
                                    for k in 0..4 {
                                        sum[k] += f64::from(p[k]);
                                        max[k] = max[k].max(p[k]);
                                    }
                                }
                                format!(
                                    "levels {levels} mean {:.1} {:.1} {:.1} a {:.1} max {} {} {} a {}",
                                    sum[0] / n,
                                    sum[1] / n,
                                    sum[2] / n,
                                    sum[3] / n,
                                    max[0],
                                    max[1],
                                    max[2],
                                    max[3]
                                )
                            }
                            Err(e) => format!("not decoded: {e}"),
                        };
                        println!(
                            "    {:08x} {:<14} sem {:>2} sampler {:#04x} {} {}x{} fmt {} streamed {} stored levels {} bytes {}/{} flags {:#x} map_type {} img_semantic {} category {} {}",
                            t.name_hash,
                            slot_name(t.name_hash),
                            t.semantic,
                            t.sampler_state,
                            cap.name_of(i),
                            rec.width,
                            rec.height,
                            rec.format,
                            rec.streamed,
                            rec.level_count,
                            rec.data_len1,
                            rec.data_len2,
                            rec.flags,
                            rec.map_type,
                            rec.semantic,
                            rec.category,
                            tail
                        );
                    }
                    for c in &mr.constants {
                        println!(
                            "    constant {:08x} {:?} = {:?}",
                            c.name_hash, c.name, c.literal
                        );
                    }
                }
            }
        }
    }
}
