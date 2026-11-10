//! Checks the binding model (`render_iw7::bind`) against the shaders:
//! `iw7_bind_check <techsets.ff> [technique substring]`. For every pass whose
//! programs are in the zone, both are translated and each argument's target
//! is checked: a constant's buffer slot and rows exist in the stage's
//! declared buffers, a texture or sampler slot is declared; and every
//! declared buffer, texture and sampler is fed by some argument.
use std::collections::{BTreeMap, BTreeSet};

use dxbc_sm5::{Container, Options, Reflection, decode, translate_program};
use fastfile_iw7::AssetType as T;
use render_iw7::bind::{self, Target};

fn reflect(bytes: &[u8]) -> Option<Reflection> {
    let c = Container::parse(bytes).ok()?;
    let p = decode(c.shader).ok()?;
    translate_program(&p, &c, &Options::for_stage(p.stage)).ok().map(|t| t.reflection)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let (w, _) = fastfile_iw7::walk::walk_fitted(&zone);
    let prog = |ty: T, id: Option<fastfile_iw7::walk::AssetId>| id.and_then(|id| w.cap.shader_programs.get(&(ty, id.index))).map(|s| &zone.image[s.pos..s.pos + s.len]);
    let mut checked = 0;
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    let mut unfed: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped = 0;
    for ts in &w.cap.techsets {
        for (tech, passes) in ts.techniques.iter().zip(&ts.passes) {
            if !tech.1.contains(want.as_str()) {
                continue;
            }
            for pass in passes {
                let (Some(vs), Some(ps)) = (prog(T::VertexShader, pass.vertex_shader).and_then(reflect), prog(T::PixelShader, pass.pixel_shader).and_then(reflect)) else {
                    skipped += 1;
                    continue;
                };
                checked += 1;
                let mut fed: [BTreeSet<(char, u32)>; 2] = Default::default();
                for (k, arg) in pass.args.iter().enumerate() {
                    let stages: Vec<(usize, &Reflection)> = [(0, &vs), (1, &ps)].into_iter().filter(|(s, _)| arg.stage & if *s == 0 { bind::STAGE_VS } else { bind::STAGE_PS } != 0).collect();
                    for (s, r) in stages {
                        match bind::target(pass, k) {
                            Some(Target::Constant { slot, register }) => {
                                fed[s].insert(('b', u32::from(slot)));
                                match r.cbuffers.iter().find(|c| c.slot == u32::from(slot)) {
                                    None => *problems.entry(format!("type {} stage {s}: cbuffer slot not declared", arg.ty)).or_default() += 1,
                                    Some(c) if u32::from(register + bind::rows(arg)) > c.registers => *problems.entry(format!("type {} stage {s}: past the buffer's end", arg.ty)).or_default() += 1,
                                    _ => {}
                                }
                            }
                            Some(Target::Texture { slot }) => {
                                fed[s].insert(('t', u32::from(slot)));
                                if !r.textures.iter().any(|t| t.slot == u32::from(slot)) {
                                    *problems.entry(format!("type {} stage {s}: texture slot not declared", arg.ty)).or_default() += 1;
                                }
                            }
                            Some(Target::Sampler { slot }) => {
                                fed[s].insert(('s', u32::from(slot)));
                                if !r.samplers.iter().any(|t| t.slot == u32::from(slot)) {
                                    *problems.entry(format!("type {} stage {s}: sampler slot not declared", arg.ty)).or_default() += 1;
                                }
                            }
                            None => *problems.entry(format!("type {}: no target", arg.ty)).or_default() += 1,
                        }
                    }
                }
                for (s, r) in [(0, &vs), (1, &ps)] {
                    for c in &r.cbuffers {
                        if !fed[s].contains(&('b', c.slot)) {
                            *unfed.entry(format!("stage {s} cbuffer b{}", c.slot)).or_default() += 1;
                        }
                    }
                    for t in &r.textures {
                        if !fed[s].contains(&('t', t.slot)) {
                            *unfed.entry(format!("stage {s} texture t{}", t.slot)).or_default() += 1;
                        }
                    }
                    for t in &r.samplers {
                        if !fed[s].contains(&('s', t.slot)) {
                            *unfed.entry(format!("stage {s} sampler s{}", t.slot)).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    println!("{checked} passes checked, {skipped} without both programs in this zone");
    println!("argument problems: {problems:?}");
    println!("declared but fed by no argument (supplied by the engine itself): {unfed:?}");
}
