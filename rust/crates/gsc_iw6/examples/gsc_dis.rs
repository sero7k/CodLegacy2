//! Prints compiled GSC as pseudo-assembly:
//! `gsc_dis <zone.ff> [script name | --all] [--hex] [--names file] [--functions]`
//!
//! Script names are the asset names the zone stores (`maps/mp/mp_frag`, or a decimal file id).
//! Without a name the scripts are listed. `--names` reads a `token|file|builtin <id> <name>` file.
#[path = "util/zone_scripts.rs"]
#[allow(dead_code)]
mod zone_scripts;

use gsc_iw6::{NameTable, Script, ScriptBytes};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(zone_path) = args.first() else {
        eprintln!("usage: gsc_dis <zone.ff> [script | --all] [--hex] [--names file] [--functions]");
        return;
    };
    let hex = args.iter().any(|a| a == "--hex");
    let only_functions = args.iter().any(|a| a == "--functions");
    let mut names = NameTable::known();
    if let Some(i) = args.iter().position(|a| a == "--names")
        && let Some(f) = args.get(i + 1)
        && let Ok(text) = std::fs::read_to_string(f)
    {
        names.merge(&text);
    }
    let want: Option<&String> = args
        .iter()
        .skip(1)
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && (*i == 0 || args[*i] != "--names"))
        .map(|(_, a)| a);
    let zone = match zone_scripts::load(zone_path) {
        Ok(z) => z,
        Err(e) => {
            eprintln!("{zone_path}: {e}");
            return;
        }
    };
    let all = args.iter().any(|a| a == "--all");
    for s in &zone.scripts {
        let selected = all || want.is_some_and(|w| *w == s.name);
        if !selected {
            if want.is_none() {
                println!("{}  (code {} bytes, stream {} bytes)", s.name, s.code.len(), s.stream.len());
            }
            continue;
        }
        let bytes = ScriptBytes { stream: &s.stream, code: &s.code };
        println!("; ==== script {} ====", s.name);
        match Script::parse(&bytes) {
            Ok(script) => {
                if only_functions {
                    for f in &script.functions {
                        println!("{:#06x} {:>5} bytes {:>4} instrs {} params  {}", f.offset, f.end - f.offset, f.count, f.params, names.token(&f.name));
                    }
                } else {
                    print!("{}", gsc_iw6::render(&script, &s.code, &names, hex));
                }
            }
            Err(e) => println!("; decode error: {e}"),
        }
    }
}
