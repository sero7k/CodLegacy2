//! Lowers every pixel and vertex program of a decoded `code_post_gfx` zone to
//! WGSL (`dxil_sm6::wgsl`) and validates the result with naga:
//! `jup_dxil_wgsl <code_post_gfx.zone> [--wgsl <index> <out.wgsl>]`.
//! `--from <i> --to <j>` limits the range; `PROGRESS=1` prints each index.
//! Prints the programs that failed, grouped by the reason, and the totals.
use std::collections::BTreeMap;

use fastfile_jup::shader::{self, Stage};

fn validate(src: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| format!("parse: {}", e.message()))?;
    let caps = naga::valid::Capabilities::all();
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&module)
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
        })?;
    Ok(())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = std::fs::read(a.get(1).expect("usage: jup_dxil_wgsl <zone>")).expect("read");
    let progs = shader::scan(&z);
    let pos = a.iter().position(|x| x == "--wgsl");
    let want = pos.map(|k| a[k + 1].parse::<usize>().expect("index"));
    let num = |f: &str| a.iter().position(|x| x == f).map(|k| a[k + 1].parse::<usize>().expect("number"));
    let (lo, hi) = (num("--from"), num("--to"));
    let (mut ok, mut total) = (0usize, 0usize);
    let mut why: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in progs.iter().enumerate() {
        if !matches!(p.stage, Stage::Pixel | Stage::Vertex) {
            continue;
        }
        if want.is_some_and(|w| w != i) {
            continue;
        }
        if lo.is_some_and(|l| i < l) || hi.is_some_and(|h| i >= h) {
            continue;
        }
        total += 1;
        if std::env::var_os("PROGRESS").is_some() {
            eprintln!("program {i}");
        }
        let m = match dxil_sm6::read_part(&z[p.bitcode.clone()]) {
            Ok(m) => m,
            Err(e) => {
                why.entry(format!("read: {e}")).or_default().push(i);
                continue;
            }
        };
        match dxil_sm6::wgsl::translate(&m) {
            Err(e) => why.entry(e.to_string()).or_default().push(i),
            Ok(t) => {
                if let Some(k) = pos {
                    std::fs::write(&a[k + 2], &t.wgsl).expect("write");
                }
                match validate(&t.wgsl) {
                    Ok(()) => ok += 1,
                    Err(e) => {
                        let first = e.chars().take(230).collect::<String>();
                        why.entry(first).or_default().push(i);
                    }
                }
            }
        }
    }
    let mut v: Vec<_> = why.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1.len()));
    for (k, ids) in &v {
        let some: Vec<String> = ids.iter().take(6).map(ToString::to_string).collect();
        println!("{:5} {k}   e.g. {}", ids.len(), some.join(","));
    }
    println!("{ok} of {total} lowered and validated");
}
