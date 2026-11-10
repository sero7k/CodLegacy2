//! bo2mp: exports Modern Warfare III's (2023) guns, camos, calling cards,
//! emblems and gun sounds to a standalone folder in our own converted files
//! (the same as `CodLegacy export-jup <folder>`, without the game's window code):
//!
//! ```text
//! BO2_JUP_INSTALL=<install> cargo run -p assets --example jup_export -- <folder>
//! cargo run -p assets --example jup_export -- --check <folder>
//! ```
//!
//! `jup_export --plan <plan.tsv> <scratch>` lists the byte ranges of the
//! package entries an export reads (`docs/JUP.md`), to be cut out on the
//! machine that has the game.
//!
//! On a machine that is not Windows, `JUP_OODLE_LIB` names a Kraken decoder
//! library and `JUP_OODLE_DLL` any existing file (the install is checked for
//! its DLL). See docs/JUP.md, "The exported guns".

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    if first.as_deref() == Some("--plan") {
        // jup_export --plan <plan.tsv> <scratch folder>
        let (Some(plan), Some(scratch)) = (args.next(), args.next()) else {
            eprintln!("usage: jup_export --plan <plan.tsv> <scratch folder>");
            std::process::exit(2);
        };
        match assets::plan_jup(std::path::Path::new(&plan), std::path::Path::new(&scratch), &mut |line| println!("{line}")) {
            Ok(line) => println!("{line}"),
            Err(e) => {
                eprintln!("plan failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if first.as_deref() == Some("--expand") {
        // jup_export --expand <export_out folder> <install folder>
        let (Some(from), Some(to)) = (args.next(), args.next()) else {
            eprintln!("usage: jup_export --expand <export_out folder> <install folder>");
            std::process::exit(2);
        };
        match expand(std::path::Path::new(&from), std::path::Path::new(&to)) {
            Ok(n) => println!("{n} ranges written into {to}"),
            Err(e) => {
                eprintln!("expand failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let check = first.as_deref() == Some("--check");
    let Some(dir) = (if check { args.next() } else { first }).filter(|d| !d.is_empty()) else {
        eprintln!("usage: jup_export <folder> | jup_export --check <folder>");
        std::process::exit(2);
    };
    let dir = std::path::PathBuf::from(dir);
    let out = if check {
        assets::check_jup_export(&dir)
    } else {
        assets::export_jup(&dir, &mut |line| println!("{line}"))
    };
    match out {
        Ok(line) => println!("{line}"),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Writes the ranges cut out by `export_extract.ps1` (`placed.tsv` and the
/// `pack_NNN.bin` beside it) back to their offsets in the install's parts
/// (`<install>/zone/<part>`, which are empty files of the right length, made
/// here when missing, so the packages open as the real ones do).
fn expand(from: &std::path::Path, install: &std::path::Path) -> Result<usize, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let placed = std::fs::read_to_string(from.join("placed.tsv")).map_err(|e| format!("placed.tsv: {e}"))?;
    // Parts the install does not have yet are made as empty files of the
    // length `file_list.txt` gives (sparse where the file system allows).
    let sizes: std::collections::HashMap<String, u64> = std::fs::read_to_string(install.join("file_list.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (path, size) = l.trim_end().split_once('\t')?;
            let name = path.rsplit(['\\', '/']).next()?.to_string();
            Some((name, size.trim().parse().ok()?))
        })
        .collect();
    // Every listed package part is there, whether or not a range falls in it:
    // the readers pick a package by the parts they find (sounds), so a part
    // that is missing would leave its sounds out.
    for (name, len) in sizes.iter().filter(|(n, _)| n.ends_with(".xsub")) {
        let part = install.join("zone").join(name);
        if !part.exists() {
            std::fs::File::create(&part).and_then(|f| f.set_len(*len)).map_err(|e| format!("{}: {e}", part.display()))?;
        }
    }
    let mut n = 0;
    let mut pack: Option<(String, std::fs::File)> = None;
    for line in placed.lines() {
        let f: Vec<&str> = line.trim_end().split('\t').collect();
        let [no, at, name, offset, size, _key] = f[..] else { continue };
        let bad = || format!("bad row `{line}`");
        let no: usize = no.parse().map_err(|_| bad())?;
        let at: u64 = at.parse().map_err(|_| bad())?;
        let offset: u64 = offset.parse().map_err(|_| bad())?;
        let size: usize = size.parse().map_err(|_| bad())?;
        let pack_name = format!("pack_{no:03}.bin");
        if pack.as_ref().is_none_or(|(n, _)| *n != pack_name) {
            let file = std::fs::File::open(from.join(&pack_name)).map_err(|e| format!("{pack_name}: {e}"))?;
            pack = Some((pack_name.clone(), file));
        }
        let (_, src) = pack.as_mut().ok_or("no pack")?;
        let mut buf = vec![0u8; size];
        src.seek(SeekFrom::Start(at)).and_then(|_| src.read_exact(&mut buf)).map_err(|e| format!("{pack_name}: {e}"))?;
        let part = install.join("zone").join(name);
        if !part.exists() {
            let len = sizes.get(name).copied().ok_or_else(|| format!("{name}: not in file_list.txt"))?;
            std::fs::File::create(&part).and_then(|f| f.set_len(len)).map_err(|e| format!("{}: {e}", part.display()))?;
        }
        let mut dst = std::fs::OpenOptions::new().write(true).open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        dst.seek(SeekFrom::Start(offset)).and_then(|_| dst.write_all(&buf)).map_err(|e| format!("{}: {e}", part.display()))?;
        n += 1;
    }
    Ok(n)
}
