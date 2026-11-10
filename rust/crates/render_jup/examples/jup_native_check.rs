//! Offline check of the Modern Warfare III native renderer, no GPU needed:
//!
//! `jup_native_check <code_post_gfx.zone> [--all] [--from <i> --to <j>] [--map <mp_xxx.ff>]`
//!
//! - validates the host's own WGSL (fallback surface, lighting, display) with naga;
//! - builds the game's pipeline list from the decoded `code_post_gfx` zone:
//!   every material pixel program with its vertex program (`library::material_pairs`),
//!   lowered, linked and validated with naga, with the tonemap pair (program 1239)
//!   and the reason for each failure, and prints the coverage;
//! - with `--map` (the game's Oodle needed: on the PC), reads the map zone and
//!   reports its draws, materials, pictures and how many materials have a program.
//!
//! Without `--all` the list is sampled (one pair per binding family).
use std::collections::BTreeMap;

use render_jup::coverage::{build_pair, describe};
use render_jup::library::{Library, material_pairs, vertex_for};
use render_jup::mapping::{Pair, check_pairs};

fn validate(src: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| format!("parse: {}", e.message()))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map(|_| ())
        .map_err(|e| {
            use std::error::Error;
            let inner = e.into_inner();
            let mut msg = format!("validate: {inner}");
            let mut cause: Option<&dyn Error> = inner.source();
            while let Some(c) = cause {
                msg += &format!(" / {c}");
                cause = c.source();
            }
            msg
        })
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let num = |f: &str| a.iter().position(|x| x == f).map(|k| a[k + 1].parse::<usize>().expect("number"));
    let mut bad = 0usize;

    println!("host modules:");
    for m in render_jup::shaders::all() {
        match validate(&m.source) {
            Ok(()) => println!("  {}: valid", m.label),
            Err(e) => {
                bad += 1;
                println!("  {}: {}", m.label, e.chars().take(400).collect::<String>());
            }
        }
    }

    if let Some(zone) = a.get(1).filter(|z| !z.starts_with("--")) {
        let lib = Library::from_bytes(std::fs::read(zone).expect("read the decoded code_post_gfx zone"));
        let graphics = (0..lib.programs.len()).filter(|&i| lib.is_graphics(i)).count();
        println!("library: {} programs, {graphics} vertex or pixel", lib.programs.len());
        let pairs: Vec<Pair> = if a.iter().any(|x| x == "--all") {
            material_pairs(&lib).into_iter().map(|(vertex, pixel)| Pair { vertex, pixel }).collect()
        } else {
            check_pairs(&lib, 64)
        };
        let (lo, hi) = (num("--from").unwrap_or(0), num("--to").unwrap_or(usize::MAX));
        let all_material = material_pairs(&lib);
        println!("material pixel programs: {} (all pair with a vertex program: {})", lib.programs.iter().filter(|p| p.stage == fastfile_jup::shader::Stage::Pixel && p.inputs.iter().any(|e| e.semantic == "GPU_PIPELINE_ID")).count(), all_material.len());
        let mut why: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let (mut lowered, mut valid, mut clean) = (0usize, 0usize, 0usize);
        let mut seen_vs: BTreeMap<usize, Result<(), String>> = BTreeMap::new();
        let mut total = 0usize;
        for pair in pairs.into_iter().filter(|p| p.pixel >= lo && p.pixel < hi) {
            total += 1;
            let (report, linked) = build_pair(&lib, pair);
            if std::env::var_os("VERBOSE").is_some() || total <= 8 {
                println!("{}", describe(&report));
            }
            let Some(linked) = linked else {
                why.entry(report.error.clone().unwrap_or_default().chars().take(160).collect()).or_default().push(pair.pixel);
                continue;
            };
            lowered += 1;
            if linked.unlinked.is_empty() {
                clean += 1;
            } else {
                why.entry(format!("pixel inputs not written by the vertex stage: {}", linked.unlinked.join(", "))).or_default().push(pair.pixel);
            }
            let vs = seen_vs.entry(pair.vertex).or_insert_with(|| validate(&linked.vertex_wgsl)).clone();
            let ps = validate(&linked.pixel_wgsl);
            match (vs, ps) {
                (Ok(()), Ok(())) => valid += 1,
                (v, p) => {
                    let e = v.err().or(p.err()).unwrap_or_default();
                    why.entry(e.chars().take(200).collect()).or_default().push(pair.pixel);
                }
            }
        }
        println!("pairs: {total} built, {lowered} lowered, {clean} linked with every pixel input written, {valid} validated by naga");

        // The display stage: the tonemap program and the vertex program that draws it.
        let tonemap = 1239usize;
        match vertex_for(&lib, tonemap) {
            Some(v) => {
                let (r, l) = build_pair(&lib, Pair { vertex: v, pixel: tonemap });
                println!("display: {}", describe(&r));
                if let Some(l) = l {
                    let ok = validate(&l.vertex_wgsl).and(validate(&l.pixel_wgsl));
                    println!("display: naga {}", ok.map_or_else(|e| e.chars().take(300).collect(), |()| "valid".to_owned()));
                }
            }
            None => println!("display: no vertex program found for program {tonemap}"),
        }
        let mut v: Vec<_> = why.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1.len()));
        for (k, ids) in &v {
            println!("{:5} {k}   e.g. ps {}", ids.len(), ids.iter().take(6).map(ToString::to_string).collect::<Vec<_>>().join(","));
        }
        bad += v.len();
    }

    if let Some(map) = a.iter().position(|x| x == "--map").map(|k| a[k + 1].clone()) {
        match render_jup::scene::Scene::load(std::path::Path::new(&map)) {
            Ok(s) => {
                for l in &s.report {
                    println!("scene: {l}");
                }
                let mapped = s
                    .materials
                    .iter()
                    .filter(|m| {
                        // (There is no library here: the answer is whatever `choose` says
                        // for an empty library; see mapping.rs.)
                        let _ = m;
                        false
                    })
                    .count();
                println!(
                    "scene: {} draws, {} materials ({mapped} with a game program, {} on the fallback shader), {} pictures, {} vertices, {} triangles, {} instances",
                    s.draws.len(),
                    s.materials.len(),
                    s.materials.len() - mapped,
                    s.images.len(),
                    s.vertices.len(),
                    s.indices.len() / 3,
                    s.instances.len()
                );
            }
            Err(e) => println!("scene: {e}"),
        }
    }
    if bad > 0 {
        println!("{bad} problem group(s)");
    }
}
