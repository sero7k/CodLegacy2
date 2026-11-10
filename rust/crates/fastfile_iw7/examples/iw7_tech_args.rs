//! What feeds a zone's shaders: `iw7_tech_args <techsets.ff> [technique
//! substring]` counts the passes' argument types, and for code constants
//! (by index) how many passes use each, its rows, the stages and the
//! destinations, over the techniques whose name holds the substring.
//! (The indices' names are the exe's table, not in the zones.)
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let (w, _) = fastfile_iw7::walk::walk_fitted(&zone);
    let mut types: BTreeMap<u8, usize> = BTreeMap::new();
    // index -> (passes, rows seen, stages, dests)
    let mut consts: BTreeMap<u16, (usize, std::collections::BTreeSet<(u8, u8)>, std::collections::BTreeSet<u8>, std::collections::BTreeSet<u16>)> = BTreeMap::new();
    let mut sets = 0;
    let mut passes = 0;
    let picked = || {
        w.cap.techsets.iter().flat_map(|ts| ts.techniques.iter().zip(&ts.passes)).filter(|(t, _)| t.1.contains(want.as_str())).map(|(_, p)| p)
    };
    for tech in picked() {
        sets += 1;
        for pass in tech {
            passes += 1;
            for arg in &pass.args {
                *types.entry(arg.ty).or_default() += 1;
            }
        }
    }
    // (The type that names code constants: the one whose index range is
    // small and shared by every pass; printed for each type below.)
    for &ty in types.keys() {
        consts.clear();
        for tech in picked() {
            for pass in tech {
                for arg in pass.args.iter().filter(|x| x.ty == ty) {
                    let e = consts.entry(arg.index).or_default();
                    e.0 += 1;
                    e.1.insert((arg.first_row, arg.row_count));
                    e.2.insert(arg.stage);
                    e.3.insert(arg.dest);
                }
            }
        }
        println!("type {ty}: {} arguments, {} distinct values", types[&ty], consts.len());
        if consts.len() <= 400 {
            for (i, (n, rows, stages, dests)) in &consts {
                let dests: Vec<u16> = dests.iter().copied().take(6).collect();
                println!("  value {i:5}: {n:6} passes, rows {rows:?}, stages {stages:?}, dests {dests:?}");
            }
        }
    }
    println!("{sets} techniques, {passes} passes");
    if std::env::var_os("IW7_TECH_NAMES").is_some() {
        for ts in w.cap.techsets.iter().take(40) {
            println!("  set {:?}: {:?}", ts.name, ts.techniques.iter().map(|t| t.1.as_str()).take(6).collect::<Vec<_>>());
        }
    }
}
