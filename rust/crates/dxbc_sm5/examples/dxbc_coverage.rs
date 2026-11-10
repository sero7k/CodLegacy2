//! `dxbc_coverage <dir>`: decodes and translates every `.dxbc` in a folder
//! (as a zone's shader dumper writes them) and counts what translates, by
//! stage, with the first errors: how far the translator gets on a game's
//! shaders. Each translation is also parsed and validated by naga.
fn main() {
    let dir = std::env::args().nth(1).expect("usage: dxbc_coverage <dir>");
    let mut ok = std::collections::BTreeMap::<String, (usize, usize)>::new();
    let mut errors = std::collections::BTreeMap::<String, usize>::new();
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("dir").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "dxbc")).collect();
    paths.sort();
    for p in &paths {
        let bytes = std::fs::read(p).expect("read");
        let result: Result<String, String> = (|| {
            let c = dxbc_sm5::Container::parse(&bytes).map_err(|e| format!("container: {e:?}"))?;
            let prog = dxbc_sm5::decode(c.shader).map_err(|e| format!("decode: {e:?}"))?;
            let stage = format!("{:?}", prog.stage);
            let tr = dxbc_sm5::translate_program(&prog, &c, &dxbc_sm5::Options::for_stage(prog.stage)).map_err(|e| format!("{stage}: {e:?}"))?;
            let module = naga::front::wgsl::parse_str(&tr.wgsl).map_err(|e| format!("{stage}: naga parse: {}", e.emit_to_string(&tr.wgsl).lines().next().unwrap_or_default()))?;
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(&module)
                .map_err(|e| format!("{stage}: naga validate: {:?}", e.as_inner()))?;
            Ok(stage.clone())
        })();
        match result {
            Ok(stage) => ok.entry(stage).or_default().0 += 1,
            Err(e) => {
                let stage = e.split(':').next().unwrap_or("?").to_owned();
                ok.entry(stage).or_default().1 += 1;
                let key: String = e.chars().take(90).collect();
                *errors.entry(key).or_default() += 1;
            }
        }
    }
    println!("{} programs", paths.len());
    for (stage, (good, bad)) in &ok {
        println!("  {stage}: {good} translate, {bad} do not");
    }
    let mut errs: Vec<_> = errors.into_iter().collect();
    errs.sort_by(|a, b| b.1.cmp(&a.1));
    for (e, n) in errs.iter().take(12) {
        println!("  {n:5} x {e}");
    }
}
