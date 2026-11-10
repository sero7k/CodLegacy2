//! `cargo run -p gsc_iw6_host --example iw6_script_dump -- <zone dir> <map> <out dir>`
//!
//! Writes the disassembly of every script of the map's zone set (the same set the census reads)
//! to `<out dir>/<script name with / as _>.txt`, with the built-in builtin names. Used to read how
//! the scripts call a builtin the census cannot name (`grep -n "0x823e" <out dir>/*`).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into()));
    let map = args.next().unwrap_or_else(|| "mp_frag".into());
    let out = PathBuf::from(args.next().expect("out dir"));
    std::fs::create_dir_all(&out).expect("out dir");
    let names = gsc_iw6::NameTable::known();
    let zones = ["common", "common_mp", "common_core_mp", "patch_common", "patch_common_mp", "patch_common_core_mp"];
    for z in zones.iter().map(|z| (*z).to_owned()).chain([map.clone(), format!("patch_{map}")]) {
        let p = dir.join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        let Ok(zone) = fastfile_iw6::scripts::read_zone(&p) else { continue };
        for s in zone.scripts {
            let bytes = gsc_iw6::ScriptBytes { stream: &s.stream, code: &s.code };
            if let Ok(text) = gsc_iw6::disassemble_with(&bytes, &names, false) {
                let f = format!("{}.txt", s.name.replace(['/', '\\'], "_"));
                std::fs::write(out.join(f), text).expect("write");
            }
        }
    }
}
