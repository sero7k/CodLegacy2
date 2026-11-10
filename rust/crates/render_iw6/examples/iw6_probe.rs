//! Lists what a Ghosts map draws with: its world surfaces' technique sets
//! and the slots they fill, state bits, pictures (inline or streamed, and
//! from which pak), the shader inputs and arguments of the lit techniques,
//! placed models' lighting, light grid and probes.
//!
//! `cargo run -p render_iw6 --release --example iw6_probe -- <map.ff> <zone.ff>...`

use std::collections::{BTreeMap, HashMap};

use fastfile_iw6::AssetType;
use render_iw6::pool::{Pool, key};

fn main() {
    let paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    let mut zones = Vec::new();
    let mut pool = Pool::default();
    for p in &paths {
        let t = std::time::Instant::now();
        let z = fastfile_iw6::Zone::open(p).expect("open");
        let (cap, stop) = fastfile_iw6::walk::walk_zone(&z);
        eprintln!(
            "{}: {} techsets, {} techniques, {} programs, {} materials, {} images, stop {:?} [{:.1}s]",
            p.display(),
            cap.techsets.len(),
            cap.techniques.len(),
            cap.shader_programs.len(),
            cap.materials.len(),
            cap.images.len(),
            stop.as_ref().map(|s| (s.asset_index, s.asset_type)),
            t.elapsed().as_secs_f32()
        );
        zones.push((z, cap));
    }
    for (z, cap) in zones.iter().skip(1).chain(zones.iter().take(1)) {
        pool.add(cap, &z.image);
    }
    eprintln!(
        "pool: {} techsets, {} techniques, {} programs",
        pool.techsets.len(),
        pool.techniques.len(),
        pool.programs.len()
    );
    let (z, cap) = &zones[0];
    let map = fastfile_iw6::map::read_map(cap, &z.image, &z.assets.script_strings);
    let w = map.world.as_ref().expect("world");
    eprintln!(
        "world: {} surfaces, {} verts, {} indices, ranges {:?}, {} smodels, {} sky surfaces, {} lightmaps, sun {:?}",
        w.surfaces.len(),
        w.vertices.len(),
        w.indices.len(),
        w.ranges,
        w.static_models.len(),
        w.sky_surfaces.len(),
        w.lightmaps.len(),
        map.sun
    );
    // Techsets of world surfaces.
    let mut sets: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut mats: HashMap<u32, usize> = HashMap::new();
    for s in &w.surfaces {
        let Some(m) = s.material else { continue };
        *mats.entry(m.index).or_default() += 1;
        let mr = &cap.materials[m.index as usize];
        let ts = mr.techset.map_or("<none>".to_string(), |t| key(cap.name_of(t)));
        let e = sets.entry(ts).or_default();
        e.0 += 1;
        e.1 += usize::from(s.tri_count);
    }
    for (ts, (n, tris)) in sets.iter().rev().take(400) {
        let slots: Vec<usize> = pool
            .techsets
            .get(ts)
            .map(|v| v.iter().map(|(i, _)| *i).collect())
            .unwrap_or_default();
        eprintln!("techset {ts}: {n} surfaces {tris} tris, slots {:?}", slots);
    }
    // PROBE_WGSL=<word>: the translation of the first pixel (or, with
    // PROBE_STAGE=vs, vertex) program whose name holds the word.
    if let Ok(word) = std::env::var("PROBE_WGSL") {
        let vs = std::env::var("PROBE_STAGE").is_ok_and(|v| v == "vs");
        let (ty, stage) = if vs {
            (AssetType::VertexShader, dxbc_sm5::Stage::Vertex)
        } else {
            (AssetType::PixelShader, dxbc_sm5::Stage::Pixel)
        };
        let mut names: Vec<&String> = pool
            .techniques
            .values()
            .flat_map(|t| t.passes.iter())
            .filter_map(|p| if vs { p.vertex_shader.as_ref() } else { p.pixel_shader.as_ref() })
            .filter(|n| n.contains(word.as_str()))
            .collect();
        names.sort();
        names.dedup();
        for n in names.iter().take(8) {
            eprintln!("program {n}");
        }
        if let Some(n) = names.first()
            && let Some(b) = pool.program(ty, n)
        {
            let c = dxbc_sm5::Container::parse(b).unwrap();
            let p = dxbc_sm5::decode(c.shader).unwrap();
            let t = dxbc_sm5::translate_program(&p, &c, &dxbc_sm5::Options::for_stage(stage)).unwrap();
            println!("{}", t.wgsl);
        }
        return;
    }
    if let Ok(words) = std::env::var("PROBE_TECH") {
        let mut names: Vec<&String> = pool.techniques.keys().filter(|n| words.split(',').any(|w| n.contains(w))).collect();
        names.sort();
        for n in names.iter().take(60) {
            let t = &pool.techniques[*n];
            eprintln!("tech {n}");
            for p in &t.passes {
                eprintln!("    vs {:?} ps {:?} counts {:?} decl {:?}", p.vertex_shader, p.pixel_shader, p.arg_counts, p.vertex_decl);
                for a in &p.args {
                    let what = match a.ty {
                        0 => { let (i, f, r) = a.code_const(); format!("code {} rows {f}+{r}", render_iw6::code::code_const_name(i)) }
                        4 => format!("literal {:?}", a.literal),
                        1 | 2 => format!("code tex {}", render_iw6::code::code_texture_name(a.u32())),
                        _ => format!("hash {:#x}", a.u32()),
                    };
                    eprintln!("      ty {} stage {:#x} dest {} {what}", a.ty, a.shader, a.dest);
                }
                if let Some(v) = &p.vertex_shader && let Some(b) = pool.program(AssetType::VertexShader, v) {
                    let c = dxbc_sm5::Container::parse(b).unwrap();
                    eprintln!("      vs inputs {:?}", c.inputs.iter().map(|e| format!("{}{}m{:x}", e.semantic, e.semantic_index, e.mask)).collect::<Vec<_>>());
                }
            }
        }
        for (s, slots) in &pool.techsets {
            if slots.iter().any(|(_, t)| words.split(',').any(|w| t.contains(w))) {
                eprintln!("techset {s} (world vertex format {:?}): {:?}", pool.world_vert_format.get(s), slots.iter().filter(|(_, t)| words.split(',').any(|w| t.contains(w))).collect::<Vec<_>>());
            }
        }
        return;
    }
    // One lit world technique in detail.
    let want = std::env::var("PROBE_SET").unwrap_or_else(|_| "wc_l_sm_r0c0n0s0l0_nfwpf".into());
    if let Some(set) = pool.techsets.get(&key(&want)) {
        for (slot, tn) in set.iter().filter(|(s, _)| *s < 12 || *s == 44) {
            let Some(t) = pool.technique(tn) else {
                eprintln!("  slot {slot}: {tn} NOT IN POOL");
                continue;
            };
            eprintln!("  slot {slot}: {tn} flags {:#x}", t.flags);
            for (pi, p) in t.passes.iter().enumerate() {
                eprintln!(
                    "    pass {pi}: vs {:?} ps {:?} hs {:?} decl {:?} counts {:?}",
                    p.vertex_shader, p.pixel_shader, p.hull_shader, p.vertex_decl, p.arg_counts
                );
                for a in &p.args {
                    let what = match a.ty {
                        0 | 4 => {
                            let (i, f, r) = a.code_const();
                            if a.ty == 0 {
                                format!("code {} rows {f}+{r}", render_iw6::code::code_const_name(i))
                            } else {
                                format!("literal {:?}", a.literal)
                            }
                        }
                        1 | 2 => format!("code tex {}", render_iw6::code::code_texture_name(a.u32())),
                        _ => format!("hash {:#x}", a.u32()),
                    };
                    eprintln!("      ty {} stage {:#x} dest {} {what}", a.ty, a.shader, a.dest);
                }
                for (ty, n) in [
                    (AssetType::VertexShader, &p.vertex_shader),
                    (AssetType::PixelShader, &p.pixel_shader),
                ] {
                    let Some(n) = n else { continue };
                    let Some(b) = pool.program(ty, n) else {
                        eprintln!("      {n}: no program");
                        continue;
                    };
                    let c = dxbc_sm5::Container::parse(b).unwrap();
                    eprintln!(
                        "      {n}: inputs {:?}",
                        c.inputs
                            .iter()
                            .map(|e| format!("{}{}r{}m{:x}", e.semantic, e.semantic_index, e.register, e.mask))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    // Materials: state bits of a few.
    let mut seen = 0;
    for (&mi, &n) in &mats {
        if seen > 12 {
            break;
        }
        let m = &cap.materials[mi as usize];
        seen += 1;
        let words: Vec<String> = m
            .state_bits
            .iter()
            .map(|b| {
                let w = |k: usize| u32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]]);
                format!("{:08x} {:08x} {:08x} | {:02x?}", w(0), w(1), w(2), &b[12..28])
            })
            .collect();
        eprintln!(
            "material {} ({n} surfs) sort {} region {} techset {:?} entry {:?}\n   bits {:?}\n   textures {:?}\n   consts {:?}",
            m.name,
            m.sort_key,
            m.camera_region,
            m.techset.map(|t| cap.name_of(t).to_string()),
            m.state_bits_entry,
            words,
            m.textures
                .iter()
                .map(|t| (
                    format!("{:#x}", t.name_hash),
                    t.sampler_state,
                    t.semantic,
                    t.image.map(|i| {
                        let r = &cap.images[i.index as usize];
                        format!("{} f{} {}x{} s{} inl{}", r.name, r.format, r.width, r.height, r.streamed, r.pixels.is_some())
                    })
                ))
                .collect::<Vec<_>>(),
            m.constants.iter().map(|c| (c.name.clone(), c.literal)).collect::<Vec<_>>()
        );
    }
    // Which paks the world's colour maps stream from (by triangles; the
    // smallest part's pak and the largest's).
    {
        let mut by_pak: BTreeMap<(u16, usize), (usize, usize, u64)> = BTreeMap::new();
        for s in &w.surfaces {
            let Some(m) = s.material else { continue };
            let mr = &cap.materials[m.index as usize];
            for t in mr.textures.iter().filter(|t| t.semantic == 2 || t.semantic == 5 || t.semantic == 8) {
                let Some(i) = t.image else { continue };
                let r = &cap.images[i.index as usize];
                for (k, f) in r.stream_files.iter().enumerate() {
                    let Some(f) = f else { continue };
                    let e = by_pak.entry((f.file_index, k)).or_default();
                    e.0 += usize::from(s.tri_count);
                    e.1 += 1;
                    e.2 += f.offset_end - f.offset;
                }
            }
        }
        for ((p, k), (tris, n, bytes)) in &by_pak {
            eprintln!("pak {p} part {k}: {tris} tris, {n} refs, {} MB", bytes / 1_000_000);
        }
    }
    // Images.
    let mut inline = 0;
    let mut streamed = 0;
    let mut refs = 0;
    let mut paks: BTreeMap<u16, (usize, u64)> = BTreeMap::new();
    for r in &cap.images {
        if fastfile_iw6::shared::is_reference(r) {
            refs += 1;
        } else if r.streamed {
            streamed += 1;
            for f in r.stream_files.iter().flatten() {
                let e = paks.entry(f.file_index).or_default();
                e.0 += 1;
                e.1 = e.1.max(f.offset_end);
            }
        } else {
            inline += 1;
        }
    }
    eprintln!("images: {inline} inline, {streamed} streamed, {refs} references; paks (index: parts, max end) {paks:?}");
    for (i, (p, s)) in w.lightmaps.iter().enumerate().take(4) {
        let d = |id: Option<fastfile_iw6::walk::AssetId>| {
            id.map(|i| {
                let r = &cap.images[i.index as usize];
                format!("{} f{} {}x{} streamed {} inline {}", r.name, r.format, r.width, r.height, r.streamed, r.pixels.is_some())
            })
        };
        eprintln!("lightmap {i}: {:?} / {:?}", d(*p), d(*s));
    }
    let g = &cap.gfx_worlds[0];
    for (i, p) in g.reflection_probes.iter().enumerate().take(4) {
        let r = p.image.map(|i| &cap.images[i.index as usize]);
        eprintln!(
            "probe {i}: {:?} {:?}",
            p.origin,
            r.map(|r| (r.name.clone(), r.format, r.width, r.map_type, r.level_count, r.streamed, r.pixels.is_some()))
        );
    }
    let sky: Vec<_> = w
        .sky_images
        .iter()
        .flatten()
        .map(|i| {
            let r = &cap.images[i.index as usize];
            (r.name.clone(), r.format, r.width, r.map_type, r.streamed, r.pixels.is_some())
        })
        .collect();
    eprintln!("sky images {sky:?}");
    let zero = w
        .static_models
        .iter()
        .filter(|m| m.ground_lighting[..3].iter().all(|v| *v == 0.0))
        .count();
    eprintln!("smodels: {} with zero ground light of {}", zero, w.static_models.len());
    {
        let mut by: BTreeMap<(bool, String), usize> = BTreeMap::new();
        for (i, sm) in w.static_models.iter().enumerate() {
            let Some(mid) = sm.model else { continue };
            let xm = &cap.xmodels[mid.index as usize];
            let z = sm.ground_lighting[..3].iter().all(|v| *v == 0.0);
            let vl = g.smodel_vertex_lighting.get(i).copied().flatten().is_some();
            for m in xm.materials.iter().flatten() {
                let mr = &cap.materials[m.index as usize];
                let ts = mr.techset.map_or("<none>".to_string(), |t| key(cap.name_of(t)));
                *by.entry((z, format!("{ts} vl={vl}"))).or_default() += 1;
            }
        }
        let mut v: Vec<_> = by.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        for ((z, ts), n) in v.iter().take(30) {
            eprintln!("  smodel material zero_ground={z} {ts}: {n}");
        }
    }
    for m in w.static_models.iter().take(3) {
        eprintln!("  smodel flags {:#x} ground {:?}", m.flags, m.ground_lighting);
    }
    eprintln!("light grid {:?}", g.light_grid.as_ref().map(|l| (l.table_version, l.palette_version, l.color_count, l.palette_entry_count, l.mins, l.maxs)));
    eprintln!("tone mappings {:?}", cap.tone_mappings.values().collect::<Vec<_>>());
    eprintln!("colorization sets {:?}", cap.colorization_sets);
    eprintln!("colorizations {:?}", cap.colorizations);
    if let Some(cw) = cap.com_worlds.first() {
        eprintln!("com world: {} lights, envs {:?}", cw.primary_light_count, cw.primary_light_envs.map(|s| s.len / 10));
        if let Some(e) = cw.primary_light_envs {
            for k in 0..(e.len / 10).min(6) {
                let b = &z.image[e.pos + 10 * k..e.pos + 10 * k + 10];
                eprintln!("  env {k}: {:?}", b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
            }
        }
        if let Some(pl) = cw.primary_lights {
            for k in 0..(cw.primary_light_count as usize).min(4) {
                eprintln!("  light {k}: type {}", z.image[pl.pos + 120 * k]);
            }
        }
    }
    let mut envs: BTreeMap<u8, usize> = BTreeMap::new();
    for s in &w.surfaces { *envs.entry(s.primary_light_env).or_default() += 1; }
    eprintln!("surface light envs {envs:?}");
    for r in &cap.raw_files {
        if r.name.contains("vision") || r.name.contains("fog") || r.name.contains("createart") {
            eprintln!("rawfile {}", r.name);
            if std::env::var("PROBE_RAW").is_ok() {
                if let Some(b) = fastfile_iw6::map::raw_file(cap, &z.image, &r.name) {
                    eprintln!("{}", String::from_utf8_lossy(&b));
                }
            }
        }
    }
}
