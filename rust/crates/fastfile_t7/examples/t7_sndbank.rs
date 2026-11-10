//! bo2mp: a zone's sound banks (`SndBank`): `t7_sndbank <zone.ff>` prints
//! each bank's counts, its radverbs and its 0x184 records (ambient rooms)
//! as text and floats, and whether the inline area at 0x68.. is non-zero;
//! `--aliases <part>` the alias names holding `part`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk_with(&z, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
    // `--aliases <part>`: the alias names holding `part` (a music state's
    // `mus_<state>_intro`, a VO line), then stop.
    if let Some(i) = a.iter().position(|x| x == "--aliases") {
        let part = a.get(i + 1).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let mut names: Vec<&str> = w.capture.sound_aliases.iter().map(|r| r.name.as_str()).filter(|n| n.to_ascii_lowercase().contains(&part)).collect();
        names.sort_unstable();
        names.dedup();
        for n in names {
            println!("{n}");
        }
        return;
    }
    let img = &z.image;
    let u32_at = |p: usize| u32::from_le_bytes(img[p..p + 4].try_into().unwrap());
    let f32_at = |p: usize| f32::from_le_bytes(img[p..p + 4].try_into().unwrap());
    let cstr = |p: usize, n: usize| String::from_utf8_lossy(&img[p..p + n]).split('\0').next().unwrap_or("").to_owned();
    for wk in w.walked.iter().filter(|x| x.ty == fastfile_t7::AssetType::Sound) {
        let h = wk.start;
        println!("bank {:?} at {h:#x}..{:#x}", wk.name, wk.end);
        let names = ["aliases", "radverbs", "ducks", "rooms(0x184)"];
        for (k, off) in [0x20usize, 0x38, 0x48, 0x58].iter().enumerate() {
            println!("  {} {}", names[k], u32_at(h + off));
        }
        let nz = (0x68..0x220a0).filter(|&o| img[h + o] != 0).count();
        println!("  inline 0x68..0x220a0 non-zero bytes: {nz}");
        if a.get(2).is_some() {
            let (s, e) = (0x68usize, 0x220a0usize);
            let first = (s..e).find(|&o| img[h + o] != 0);
            println!("  first non-zero inline at {first:x?}");
        }
        // Every 64-byte name in the bank's data that a needle names, with
        // the record around it as words.
        for needle in a.iter().skip(3) {
            let nb = needle.as_bytes();
            let mut p = wk.start;
            while p + nb.len() < wk.end {
                if &img[p..p + nb.len()] == nb {
                    println!("  `{needle}` at +{:#x}", p - h);
                    for r in 0..(0x184 / 16 + 1) {
                        let q = p + r * 16;
                        let words: Vec<String> = (0..4).map(|k| {
                            let v = u32_at(q + 4 * k);
                            let f = f32_at(q + 4 * k);
                            if (1e-4..1e6).contains(&f.abs()) { format!("{f:.4}") } else { format!("{v:#x}") }
                        }).collect();
                        println!("    +{:#05x} {:<48} {}", r * 16, words.join(" "), cstr(q, 16).replace(|c: char| !c.is_ascii_graphic(), "."));
                    }
                    p += nb.len();
                } else {
                    p += 1;
                }
            }
        }
        if a.get(2).is_some_and(|v| v == "inline") {
            for o in (0x4090..0x4200).step_by(16) {
                println!("  inline +{o:#x}: {:02x?}", &img[h + o..h + o + 16]);
            }
        }
        if std::env::var("T7_SND_STRINGS").is_ok() {
            let mut p = wk.start;
            while p < wk.end {
                let mut q = p;
                while q < wk.end && img[q].is_ascii_graphic() { q += 1; }
                if q - p >= 5 && img[q] == 0 { println!("  str +{:#x} {}", p - h, String::from_utf8_lossy(&img[p..q])); }
                p = q + 1;
            }
        }
        let _ = (f32_at, cstr);
    }
    for r in &w.capture.sound_rooms {
        println!("room {} hash {:#x} radverb {} ({:#x}) dry {} wet {} tone {:#x} snapshot {} words 0x44 {:#x}", r.name, r.hash, r.radverb, r.radverb_hash, r.dry, r.wet, r.tone_id, r.snapshot, u32::from_le_bytes(r.record[0x44..0x48].try_into().unwrap()));
    }
    for r in &w.capture.sound_radverbs {
        println!("radverb {} {:#x} {:?}", r.name, r.id, r.values);
    }
    if std::env::var("T7_SND_IDS").is_ok() {
        for r in &w.capture.sound_aliases {
            println!("alias {} id {:#x}", r.name, u32::from_le_bytes(r.record[8..12].try_into().unwrap()));
        }
    }
    // The records follow the header in stream order; print the raw region
    // ranges walked for sound so the records can be found.
}
