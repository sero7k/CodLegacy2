//! Lists Ghosts' full-screen post techniques whose name holds a word:
//! per pass the code textures and code constants its arguments name.
//!
//! `cargo run -p render_iw6 --example iw6_post_probe -- <word> <zone.ff>...`
//! (the zones: `techsets_code_post_gfx_mp.ff`, `patch_code_post_gfx_mp.ff`, ...)

use fastfile_iw6::AssetType;
use render_iw6::bind::{self, MaterialView, TexSource};
use render_iw6::code::{code_const_name, code_texture_name};
use render_iw6::pool::Pool;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let word = a[0].to_ascii_lowercase();
    let mut pool = Pool::default();
    let mut zones = Vec::new();
    for p in &a[1..] {
        let z = fastfile_iw6::Zone::open(std::path::Path::new(p)).expect("open");
        let (cap, _) = fastfile_iw6::walk::walk_zone(&z);
        zones.push((z, cap));
    }
    for (z, cap) in zones.iter() {
        pool.add(cap, &z.image);
    }
    let mut names: Vec<&String> = pool.techniques.keys().filter(|k| k.contains(&word)).collect();
    names.sort();
    for n in names {
        let t = &pool.techniques[n];
        for (pi, p) in t.passes.iter().enumerate() {
            let b = bind::resolve(p, &MaterialView { textures: &[], constants: &[] });
            let mut texs: Vec<String> = b
                .ps
                .textures
                .iter()
                .map(|(s, src)| match src {
                    TexSource::Code(c) => format!("t{s}={}", code_texture_name(*c)),
                    o => format!("t{s}={o:?}"),
                })
                .collect();
            texs.sort();
            let mut cs: Vec<String> = b
                .cbuffer
                .iter()
                .filter_map(|w| match &w.src {
                    bind::CbSource::Code { index, rows, .. } => Some(format!("{}x{}", code_const_name(*index), rows)),
                    _ => None,
                })
                .collect();
            cs.sort();
            cs.dedup();
            let ps = p.pixel_shader.clone().unwrap_or_default();
            if let Ok(dir) = std::env::var("PROBE_WGSL")
                && let (Some(vs_name), Some(ps_b)) = (p.vertex_shader.as_ref(), pool.program(AssetType::PixelShader, &ps))
                && let Some(vs_b) = pool.program(AssetType::VertexShader, vs_name)
                && let (Ok(vc), Ok(pc)) = (dxbc_sm5::Container::parse(vs_b), dxbc_sm5::Container::parse(ps_b))
            {
                let mut po = dxbc_sm5::Options::for_stage(dxbc_sm5::Stage::Pixel);
                po.varyings = dxbc_sm5::link(&vc.outputs, &pc.inputs);
                if let Ok(t) = dxbc_sm5::translate(ps_b, &po) {
                    let _ = std::fs::write(format!("{dir}/{n}.wgsl"), &t.wgsl);
                }
            }
            let sz = pool.program(AssetType::PixelShader, &ps).map_or(0, |b| b.len());
            println!("{n}[{pi}] ps={ps}({sz}B)\n   tex: {}\n   const: {}", texs.join(" "), cs.join(" "));
        }
    }
}
