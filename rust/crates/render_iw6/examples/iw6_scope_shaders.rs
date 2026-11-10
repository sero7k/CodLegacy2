//! Lists the techniques whose passes read the scope / thermal / PiP engine inputs, with the
//! shaders and every engine constant and texture they take:
//! `iw6_scope_shaders <zone.ff>...` (lens, thermal, distortion constants; the PiP scene texture).
use render_iw6::bind::arg;
use render_iw6::names::{CODE_CONSTS, CODE_TEXTURES};
use render_iw6::pool::Pool;

fn main() {
    let mut pool = Pool::default();
    for p in std::env::args().skip(1) {
        let z = fastfile_iw6::Zone::open(std::path::Path::new(&p)).expect("open");
        let (cap, _) = fastfile_iw6::walk::walk_zone(&z);
        pool.add(&cap, &z.image);
    }
    // DUMP_PS=<pixel shader name>: its WGSL and reflection (how it uses the inputs).
    if let Ok(n) = std::env::var("DUMP_PS") {
        let b = pool.program(fastfile_iw6::AssetType::PixelShader, &render_iw6::pool::key(&n)).expect("shader not in the zones");
        let c = dxbc_sm5::Container::parse(b).expect("container");
        let prog = dxbc_sm5::decode(c.shader).expect("decode");
        let t = dxbc_sm5::translate_program(&prog, &c, &dxbc_sm5::Options::for_stage(prog.stage)).expect("translate");
        println!("{}", t.wgsl);
        eprintln!("{:#?}", t.reflection);
        return;
    }
    let interesting = |n: &str| n.contains("SCOPE") || n.contains("THERMAL") || n.contains("DISTORTION_SAMPLE") || n.contains("PIP");
    if let Ok(w) = std::env::var("NAMES") {
        // NAMES=<word>: every technique and techset name holding the word.
        for k in pool.techniques.keys().chain(pool.techsets.keys()).filter(|k| k.contains(&w)) {
            println!("{k}");
        }
        return;
    }
    if let Ok(slot) = std::env::var("SLOT") {
        // SLOT=<n>: the techniques every techset holds in that slot (distinct, with their shaders and args).
        let slot: usize = slot.parse().unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut sets = 0;
        for (_, slots) in &pool.techsets {
            for (i, n) in slots {
                if *i == slot {
                    sets += 1;
                    if seen.insert(n.clone()) {
                        if let Some(t) = pool.techniques.get(n) {
                            let p = &t.passes[0];
                            let a: Vec<String> = p.args.iter().filter_map(|a| match a.ty {
                                arg::CODE_CONST => Some(format!("c:{}", CODE_CONSTS.get(usize::from(a.code_const().0)).copied().unwrap_or("?"))),
                                arg::CODE_TEXTURE => Some(format!("t:{}", CODE_TEXTURES.get(a.u32() as usize).copied().unwrap_or("?"))),
                                _ => None,
                            }).collect();
                            println!("{n}: ps {:?} {a:?}", p.pixel_shader);
                        }
                    }
                }
            }
        }
        println!("{sets} techsets, {} distinct techniques", seen.len());
        return;
    }
    if std::env::var_os("SLOTS").is_some() {
        let mut h = std::collections::BTreeMap::<usize, usize>::new();
        for slots in pool.techsets.values() {
            for (i, _) in slots {
                *h.entry(*i).or_default() += 1;
            }
        }
        println!("{h:?}");
        return;
    }
    let mut names: Vec<_> = pool.techniques.keys().cloned().collect();
    names.sort();
    for k in names {
        let t = &pool.techniques[&k];
        for (pi, p) in t.passes.iter().enumerate() {
            let mut hits = Vec::new();
            let mut all = Vec::new();
            for a in &p.args {
                let s = match a.ty {
                    arg::CODE_CONST => {
                        let (i, r, n) = a.code_const();
                        format!("c:{}[{r}+{n}]@{}", CODE_CONSTS.get(usize::from(i)).copied().unwrap_or("?"), a.dest)
                    }
                    arg::CODE_TEXTURE => format!("t:{}@{}", CODE_TEXTURES.get(a.u32() as usize).copied().unwrap_or("?"), a.dest),
                    arg::CODE_SAMPLER => format!("s:{}@{}", a.u32(), a.dest),
                    arg::LITERAL_CONST => format!("lit{:?}@{}", a.literal.unwrap_or_default(), a.dest),
                    arg::MATERIAL_CONST => format!("mc:{:08x}@{}", a.u32(), a.dest),
                    arg::MATERIAL_TEXTURE => format!("mt:{:08x}@{}", a.u32(), a.dest),
                    _ => continue,
                };
                if interesting(&s) {
                    hits.push(s.clone());
                }
                all.push(s);
            }
            if !hits.is_empty() {
                println!("{} pass {pi} vs {:?} ps {:?}", t.name, p.vertex_shader, p.pixel_shader);
                println!("   hits {hits:?}");
                if std::env::var_os("ALL").is_some() {
                    println!("   args {all:?}");
                }
            }
        }
    }
}
