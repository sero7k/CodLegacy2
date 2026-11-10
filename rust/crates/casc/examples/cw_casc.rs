//! `cw_casc list <install> [filter]` prints `path<TAB>size<TAB>R|-` for every TVFS file.
//! `cw_casc extract <install> <out dir> <path>...` writes decoded files under `<out dir>`.
//! Both only read the install.
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let usage = "usage: cw_casc list <install> [filter] | extract <install> <out dir> <path>...";
    let (cmd, install) = (a.get(1).ok_or(usage)?, Path::new(a.get(2).ok_or(usage)?));
    let s = casc::storage::Storage::open(install)?;
    match cmd.as_str() {
        "list" => {
            let filt = a.get(3).map(String::as_str).unwrap_or("");
            for f in s.files.iter().filter(|f| f.path.contains(filt)) {
                println!("{}\t{}\t{}", f.path, f.size(), if s.resident(f) { "R" } else { "-" });
            }
        }
        "extract" => {
            let out = Path::new(a.get(3).ok_or(usage)?);
            for p in &a[4..] {
                let b = s.read(p)?;
                let dst = out.join(p);
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::write(&dst, &b)?;
                println!("{p}: {} bytes -> {}", b.len(), dst.display());
            }
        }
        _ => return Err(usage.into()),
    }
    Ok(())
}
