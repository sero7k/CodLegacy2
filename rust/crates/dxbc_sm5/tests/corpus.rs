//! Translates every `.dxbc` under `$DXBC_DIR` (one folder per stage, as
//! `h1_tech --dump` writes them) and validates the WGSL with naga.
//! Skipped when the variable is unset.

use std::collections::BTreeMap;

fn validate(src: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| e.emit_to_string(src))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn corpus() {
    let Ok(dir) = std::env::var("DXBC_DIR") else {
        return;
    };
    let limit: usize = std::env::var("DXBC_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let mut ok = 0usize;
    let mut fail: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut n = 0usize;
    for stage in ["vs", "ps", "cs", ""] {
        let Ok(rd) = std::fs::read_dir(std::path::Path::new(&dir).join(stage)) else {
            continue;
        };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for p in files.into_iter().take(limit) {
            n += 1;
            let b = std::fs::read(&p).unwrap();
            let c = dxbc_sm5::Container::parse(&b).unwrap();
            let prog = dxbc_sm5::decode(c.shader).unwrap();
            let opts = dxbc_sm5::Options::for_stage(prog.stage);
            let res = dxbc_sm5::translate_program(&prog, &c, &opts).map_err(|e| format!("{e:?}"));
            if matches!(res, Err(ref e) if e == "NoPosition") {
                n -= 1;
                continue;
            }
            let res = res.and_then(|t| {
                validate(&t.wgsl).map_err(|e| {
                    if std::env::var("DXBC_SHOW").is_ok() {
                        eprintln!("---- {}\n{}\n{}", p.display(), t.wgsl, e);
                    }
                    e
                })
            });
            match res {
                Ok(()) => ok += 1,
                Err(e) => {
                    let key: String = e
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .chars()
                        .take(120)
                        .collect();
                    let ent = fail.entry(key).or_insert((0, p.display().to_string()));
                    ent.0 += 1;
                }
            }
        }
    }
    eprintln!("{ok}/{n} translated and valid");
    let mut v: Vec<_> = fail.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1.0));
    for (k, (c, ex)) in v.iter().take(40) {
        eprintln!("{c:6}  {k}\n        e.g. {ex}");
    }
    assert!(v.is_empty(), "{} failure kinds", v.len());
}
