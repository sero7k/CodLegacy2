//! Decodes every script of the Ghosts zones, when they are there. Looked for in `IW6_ZONE_DIR`,
//! then `CODLEGACY_GHOSTS_DIR/zone`, then `/mnt/user-data/uploads/ghosts_game_files/zone`; skips
//! (passes) when none exists.
#[path = "../examples/util/zone_scripts.rs"]
mod zone_scripts;

use std::path::PathBuf;

use gsc_iw6::{Name, Operand, Script, ScriptBytes};

const ZONES: [&str; 10] = [
    "mp_frag",
    "patch_mp_frag",
    "common_mp",
    "common_core_mp",
    "common",
    "patch_common_mp",
    "patch_common_core_mp",
    "patch_common",
    "common_bots_mp",
    "development_mp",
];

fn zone_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("IW6_ZONE_DIR") {
        candidates.push(d.into());
    }
    if let Some(d) = codlegacy_env::var_os("CODLEGACY_GHOSTS_DIR") {
        candidates.push(PathBuf::from(d).join("zone"));
    }
    candidates.push("/mnt/user-data/uploads/ghosts_game_files/zone".into());
    candidates.into_iter().find(|p| p.join("mp_frag.ff").is_file())
}

#[test]
fn every_script_decodes_exactly() {
    let Some(dir) = zone_dir() else {
        eprintln!("no Ghosts zones found; skipping");
        return;
    };
    let (mut scripts, mut functions, mut instrs) = (0, 0, 0);
    for z in ZONES {
        let path = dir.join(format!("{z}.ff"));
        if !path.is_file() {
            continue;
        }
        let zone = zone_scripts::load(path.to_str().expect("utf-8 path")).expect("zone opens");
        for s in &zone.scripts {
            let script = Script::parse(&ScriptBytes { stream: &s.stream, code: &s.code })
                .unwrap_or_else(|e| panic!("{z}/{}: {e}", s.name));
            let problems = script.validate();
            assert!(problems.is_empty(), "{z}/{}: {problems:?}", s.name);
            scripts += 1;
            functions += script.functions.len();
            instrs += script.instrs.len();
        }
    }
    eprintln!("decoded {scripts} scripts, {functions} functions, {instrs} instructions");
    assert!(scripts >= 300);
}

#[test]
fn mp_frag_main_script() {
    let Some(dir) = zone_dir() else { return };
    let zone = zone_scripts::load(dir.join("mp_frag.ff").to_str().expect("utf-8 path"))
        .expect("zone opens");
    let s = zone.scripts.iter().find(|s| s.name == "maps/mp/mp_frag").expect("map script");
    let script = Script::parse(&ScriptBytes { stream: &s.stream, code: &s.code }).expect("decodes");
    assert_eq!(script.functions.len(), 20);
    assert_eq!(script.functions[0].name, Name::Id(20445));
    // `main` starts with the three precache / fx calls.
    let main = script.body(&script.functions[0]);
    assert_eq!(main[0].mnemonic, "check_params");
    assert_eq!(
        main[1].operands,
        [Operand::Far { file: Name::Id(1399), func: Name::Id(20445) }]
    );
    assert!(script.instrs.iter().any(|i| i.operands.contains(&Operand::Str("compass_map_mp_frag".to_owned()))));
}
