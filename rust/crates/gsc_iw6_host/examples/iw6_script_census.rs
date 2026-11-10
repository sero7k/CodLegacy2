//! `cargo run -p gsc_iw6_host --example iw6_script_census -- <zone dir> <map> [--md]`
//!
//! For a Ghosts map zone (`mp_frag`, ...): loads the map's scripts over the common zones found
//! beside it (map, patch_<map>, common, common_mp, common_core_mp and their patches), lists
//! every builtin id the scripts call with its host status (implemented / approximate / no-op /
//! UNIMPLEMENTED), then boots the map script for a few seconds and prints the builtins the
//! running script actually hit that the host turned away.

use std::path::PathBuf;

use gsc_iw6_host::{boot, census};
use gsc_iw6_vm::{file_name, ScriptSet};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into()));
    let map = args.next().unwrap_or_else(|| "mp_frag".into());
    let md = args.any(|a| a == "--md");

    let mut scripts: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
    let zones = ["common", "common_mp", "common_core_mp", "patch_common", "patch_common_mp", "patch_common_core_mp"];
    for z in zones.iter().map(|z| (*z).to_owned()).chain([map.clone(), format!("patch_{map}")]) {
        let p = dir.join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        match fastfile_iw6::scripts::read_zone(&p) {
            Ok(zone) => {
                eprintln!("{z}: {} scripts", zone.scripts.len());
                for s in zone.scripts {
                    scripts.retain(|(n, _, _)| *n != s.name);
                    scripts.push((s.name, s.stream, s.code));
                }
            }
            Err(e) => eprintln!("{z}: {e}"),
        }
    }
    let mut set = ScriptSet::new();
    for (n, st, code) in &scripts {
        if let Err(e) = set.add_bytes(file_name(n), st, code) {
            eprintln!("{n}: {e}");
        }
    }
    use gsc_iw6_vm::{FuncRef, Name};
    let mut entries: Vec<FuncRef> = set.resolve(&Name::Text(format!("maps/mp/{map}")), &Name::Id(20445)).into_iter().collect();
    for (i, p) in set.iter() {
        let n = p.name.to_string();
        if n.starts_with(&format!("maps/mp/{map}")) || n.starts_with(&format!("maps/createfx/{map}")) {
            entries.extend((0..p.script.functions.len()).map(|f| FuncRef { script: i, func: f as u32 }));
        }
    }
    let reach = gsc_iw6_vm::reachable(&set, &entries);
    let reach_rows = census::rows(&gsc_iw6_vm::builtin_uses_within(&set, &reach));
    let all_rows = census::rows(&gsc_iw6_vm::builtin_uses(&set));
    let title = format!("Ghosts script builtin census: {map} ({} scripts)", set.len());
    if md {
        print!("{}", census::markdown(&reach_rows, &format!("{title}: reachable from the map's main ({} functions)", reach.len())));
        println!();
        print!("{}", census::markdown(&all_rows, &format!("{title}: every script loaded (animscripts, bots and player code included)")));
    } else {
        for (label, rows) in [("reachable from the map's main", &reach_rows), ("every script loaded", &all_rows)] {
            let t = census::tally(rows);
            println!("{title}: {label}\nfunctions {:?} methods {:?} (implemented, approximate, no-op, unimplemented)", t[0], t[1]);
            for r in rows {
                println!(
                    "{:#06x} {:8} {:32} {:13} x{:<5} {}",
                    r.id,
                    if r.method { "method" } else { "function" },
                    r.name.unwrap_or("(unnamed)"),
                    r.status.label(),
                    r.calls,
                    r.scripts.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                );
            }
        }
    }

    // What the scripts reachable from the map's main name (the assets lane converts these).
    {
        let lits = gsc_iw6_vm::string_literals(&set, Some(&reach));
        let fx: Vec<&String> = lits.iter().filter(|l| l.to_ascii_lowercase().starts_with("fx/") || l.to_ascii_lowercase().starts_with("vfx/")).collect();
        println!("\nreachable functions {} of {}; effect paths named: {}", reach.len(), set.iter().map(|(_, p)| p.script.functions.len()).sum::<usize>(), fx.len());
        for f in &fx {
            println!("  fx {f}");
        }
    }

    // The dynamic half: boot the map's own script and run it.
    let zone_path = dir.join(format!("{map}.ff"));
    let Ok(zone) = fastfile_iw6::Zone::open(&zone_path) else {
        eprintln!("{map}.ff not readable: no dynamic run");
        return;
    };
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let m = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let blocks = fastfile_iw6::scripts::raw_blocks(&m.entities);
    let tables: Vec<_> = cap.string_tables.iter().map(|t| (t.name.clone(), t.columns, t.rows, t.cells.clone())).collect();
    let data = boot::BootData { map: &map, scripts: &scripts, tables: &tables, entities: &blocks };
    match boot::boot(&data) {
        Ok((mut vm, rep)) => {
            let mut errors = Vec::new();
            for _ in 0..200 {
                errors.extend(vm.step(50).errors.iter().map(|e| e.to_string()));
                vm.host_mut().take_effects();
            }
            println!("\nboot: {rep:?}");
            println!("script errors in 10 s: {}", errors.len());
            for e in errors.iter().take(10) {
                println!("  {e}");
            }
            println!("builtins hit and turned away: {:?}", vm.unimplemented_builtins());
        }
        Err(e) => println!("boot failed: {e}"),
    }
}
