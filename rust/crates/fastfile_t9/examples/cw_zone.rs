//! `cw_zone <install> <out dir> <zone>...` (e.g. `mp_sm_central`): reads `zone/<zone>.ff` from the
//! CASC install (read-only), decodes it with the game's Oodle DLL (Windows; elsewhere
//! `$JUP_OODLE_LIB`), and writes `<out dir>/<zone>.zone` (the decoded image) and
//! `<out dir>/<zone>.txt` (header, block list, the image's printable strings). A zone that fails
//! does not stop the others: its raw `<zone>.ff` and `<zone>.fd` are written instead (for
//! staging), and the run ends with an error naming the failures.
use std::fmt::Write as _;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        return Err("usage: cw_zone <install> <out dir> <zone>...".into());
    }
    let (install, out) = (Path::new(&a[1]), Path::new(&a[2]));
    std::fs::create_dir_all(out)?;
    let s = casc::storage::Storage::open(install)?;
    let oodle = fastfile_jup::oodle::load(install)?;
    let mut failed = Vec::new();
    for z in &a[3..] {
        if let Err(e) = one(&s, oodle.as_ref(), out, z) {
            eprintln!("{z}: {e}; writing the raw .ff/.fd for staging");
            for ext in ["ff", "fd"] {
                if let Ok(raw) = s.read(&format!("zone/{z}.{ext}")) {
                    std::fs::write(out.join(format!("{z}.{ext}")), raw)?;
                }
            }
            failed.push(z.clone());
        }
    }
    if !failed.is_empty() {
        return Err(format!("not decoded: {}", failed.join(", ")).into());
    }
    Ok(())
}

fn one(
    s: &casc::storage::Storage,
    oodle: &dyn fastfile_jup::oodle::Decompressor,
    out: &Path,
    z: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    {
        let ff = s.read(&format!("zone/{z}.ff"))?;
        let h = fastfile_t9::taff::header(&ff)?;
        let bl = fastfile_t9::taff::blocks(&ff)?;
        let img = fastfile_t9::taff::decode(&ff, oodle)?;
        std::fs::write(out.join(format!("{z}.zone")), &img)?;
        let mut t = format!("{h:?}\nblocks {}\n", bl.len());
        let mut run = Vec::new();
        for (i, &c) in img.iter().enumerate() {
            if (0x20..0x7f).contains(&c) {
                run.push(c);
            } else {
                if run.len() >= 6 {
                    writeln!(t, "{:#010x} {}", i - run.len(), String::from_utf8_lossy(&run))?;
                }
                run.clear();
            }
        }
        std::fs::write(out.join(format!("{z}.txt")), t)?;
        println!("{z}: {} bytes decoded from {} blocks", img.len(), bl.len());
    }
    Ok(())
}
