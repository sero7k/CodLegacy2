//! Techniques of a zone: each pass's shaders, constant-buffer sizes and
//! argument table; optionally writes every shader program (DXBC) out.
//!
//! `cargo run -p fastfile_h1 --release --example h1_tech -- <zone.ff> [filter] [--dump <dir>]`
//!
//! `filter` keeps techniques whose name contains it. With `--dump`, each
//! shader program goes to `<dir>/<type>/<name>.dxbc`.

use fastfile_h1::AssetType;

fn main() {
    let mut path = None;
    let mut filter = String::new();
    let mut dump: Option<std::path::PathBuf> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--dump" {
            dump = it.next().map(Into::into);
        } else if path.is_none() {
            path = Some(std::path::PathBuf::from(a));
        } else {
            filter = a;
        }
    }
    let path = path.expect("zone path");
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
    if let Some(s) = stop {
        eprintln!(
            "walk stopped at asset {} ({:?}): {}",
            s.asset_index, s.asset_type, s.error
        );
    }
    eprintln!(
        "{} techsets, {} techniques, {} shader programs",
        cap.techsets.len(),
        cap.techniques.len(),
        cap.shader_programs.len()
    );
    for t in cap.techniques.iter().filter(|t| t.name.contains(&filter)) {
        println!(
            "== {} flags {:#06x} passes {}",
            t.name,
            t.flags,
            t.pass_info.len()
        );
        for (i, p) in t.pass_info.iter().enumerate() {
            println!(
                "  pass {i}: vs {:?} ps {:?} decl {:?} hs {:?} ds {:?}",
                p.vertex_shader, p.pixel_shader, p.vertex_decl, p.hull_shader, p.domain_shader
            );
            println!(
                "    counts {:?} sizes {:?} cbufs {:?} zone {} cbf {:#x} csf {:#x} pre {} stage {:#x} mask {:#x}",
                p.arg_counts,
                p.arg_sizes,
                p.constant_buffers,
                p.zone,
                p.custom_buffer_flags,
                p.custom_sampler_flags,
                p.precompiled_index,
                p.stage_config,
                p.pixel_output_mask
            );
            for a in &p.args {
                let (idx, row, rows) = a.code_const();
                print!(
                    "    kind {:2} stage {} dest {:5}  raw {:02x?}  cc {idx} r{row}+{rows} {}",
                    a.kind,
                    a.shader,
                    a.dest,
                    a.raw,
                    match a.kind {
                        0 => fastfile_h1::code_source::code_const_name(idx),
                        1 | 2 => fastfile_h1::code_source::code_texture_name(a.word()),
                        _ => "",
                    }
                );
                if let Some(l) = a.literal {
                    print!("  lit {l:?}");
                }
                println!();
            }
        }
    }
    if let Some(dir) = dump {
        let mut n = 0;
        for (ty, name, span) in &cap.shader_programs {
            let sub = match ty {
                AssetType::VertexShader => "vs",
                AssetType::PixelShader => "ps",
                AssetType::HullShader => "hs",
                AssetType::DomainShader => "ds",
                AssetType::ComputeShader => "cs",
                _ => "other",
            };
            let d = dir.join(sub);
            std::fs::create_dir_all(&d).expect("dump dir");
            let file = name.replace(['/', '\\', ':'], "_");
            let bytes = &zone.image[span.pos..span.pos + span.len];
            std::fs::write(d.join(format!("{file}.dxbc")), bytes).expect("write");
            n += 1;
        }
        eprintln!("wrote {n} programs");
    }
}
