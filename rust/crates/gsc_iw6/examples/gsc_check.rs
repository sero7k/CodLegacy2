//! Decodes every script of the given zones and reports counts; exit code 1 on any problem:
//! `gsc_check <zone.ff>...`
#[path = "util/zone_scripts.rs"]
#[allow(dead_code)]
mod zone_scripts;

use std::collections::BTreeMap;

use gsc_iw6::{Script, ScriptBytes};

fn main() {
    let mut total = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut ops: BTreeMap<u8, usize> = BTreeMap::new();
    let mut bad = 0;
    for path in std::env::args().skip(1) {
        let zone = match zone_scripts::load(&path) {
            Ok(z) => z,
            Err(e) => {
                println!("{path}: cannot open: {e}");
                bad += 1;
                continue;
            }
        };
        let (mut funcs, mut instrs, mut unknown, mut problems) = (0, 0, 0, 0);
        for s in &zone.scripts {
            match Script::parse(&ScriptBytes { stream: &s.stream, code: &s.code }) {
                Ok(script) => {
                    funcs += script.functions.len();
                    instrs += script.instrs.len();
                    for i in &script.instrs {
                        *ops.entry(i.op).or_default() += 1;
                    }
                    for p in script.validate() {
                        println!("  {} @{:#x}: {}", s.name, p.offset, p.message);
                        problems += 1;
                    }
                }
                Err(e) => {
                    println!("  {}: {e}", s.name);
                    if matches!(e, gsc_iw6::Error::Unknown { .. }) {
                        unknown += 1;
                    }
                    problems += 1;
                }
            }
        }
        println!(
            "{path}: walked {}/{} assets{}; scripts listed {}, decoded {}; functions {funcs}, instructions {instrs}, unknown-opcode scripts {unknown}, problems {problems}",
            zone.walked,
            zone.assets,
            zone.stop.as_ref().map(|s| format!(" (walk stopped: {s})")).unwrap_or_default(),
            zone.listed,
            zone.scripts.len(),
        );
        total.0 += zone.scripts.len();
        total.1 += funcs;
        total.2 += instrs;
        total.3 += unknown;
        total.4 += problems;
    }
    println!(
        "TOTAL: scripts {}, functions {}, instructions {}, unknown opcodes {}, problems {}; distinct opcodes used {}",
        total.0, total.1, total.2, total.3, total.4, ops.len()
    );
    if total.4 > 0 || bad > 0 {
        std::process::exit(1);
    }
}
