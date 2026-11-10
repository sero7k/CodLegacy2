//! A Black Ops III zone's effects (`walk::fx` capture): `t7_fx <zone.ff>
//! [name substring]` prints each matching effect's elements: type, counts,
//! visuals, the effects it names, and the element's first 0xd0 bytes as
//! words (`T7_FX_RAW=1`, as i32 / f32), its samples (`T7_FX_SAMPLES=1`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let w = fastfile_t7::walk::walk(&z);
    let raw = std::env::var_os("T7_FX_RAW").is_some();
    let samples = std::env::var_os("T7_FX_SAMPLES").is_some();
    // T7_FX_MATS=1: each effect material once, its technique set and textures.
    if std::env::var_os("T7_FX_MATS").is_some() {
        let mut seen = std::collections::BTreeSet::new();
        for fx in w.capture.fx.iter().filter(|f| f.name.contains(&want)) {
            for e in &fx.elems {
                if e.raw[0xc8] > 6 { continue; }
                for v in &e.visuals {
                    if !seen.insert(v.clone()) { continue; }
                    match w.capture.material(v.trim_start_matches(',')) {
                        Some(m) => {
                            println!("{v}: {:?} {:?}", m.techset, m.textures);
                            for (slot, g) in &m.globals {
                                let fl: Vec<String> = g.chunks(4).map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap()))).collect();
                                println!("    globals[{slot}] {}", fl.join(" "));
                            }
                            for (_, t) in &m.textures {
                                if let Some(i) = w.capture.image(t.trim_start_matches(',')) {
                                    println!("    {t}: {}x{} fmt {} levels {} zone {:?} fallback {:?} xpak {:?}", i.width, i.height, i.format, i.levels, i.pixels.map(|p| p.1), i.fallback.map(|p| p.1), i.xpak.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>());
                                }
                            }
                        }
                        None => println!("{v}: (not in zone)"),
                    }
                }
            }
        }
        return;
    }
    // T7_FX_DRAW=1: per element, the words the exe's sprite record writer
    // reads (`FUN_141573090`): flags (+0, +4), pivot (+0x228, +0x22c),
    // alpha scale (+0x230), soft depth (+0x234), view-angle fade (+0x238, +0x23c).
    if std::env::var_os("T7_FX_DRAW").is_some() {
        for fx in w.capture.fx.iter().filter(|f| f.name.contains(&want)) {
            for (i, e) in fx.elems.iter().enumerate() {
                let r = &e.raw;
                let u = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                println!(
                    "{}#{i} t{} flags {:#010x} {:#010x} pivot {} {} alpha {} soft {} fade {} {} vis {:?}",
                    fx.name.rsplit('/').next().unwrap_or(""),
                    r[0xc8],
                    u(0),
                    u(4),
                    f(0x228),
                    f(0x22c),
                    f(0x230),
                    f(0x234),
                    u(0x238),
                    u(0x23c),
                    e.visuals
                );
            }
        }
        return;
    }
    // T7_FX_TABLE=1: one line per element, its first 0xc8 bytes as words.
    if std::env::var_os("T7_FX_TABLE").is_some() {
        for fx in w.capture.fx.iter().filter(|f| f.name.contains(&want)) {
            for (i, e) in fx.elems.iter().enumerate() {
                let words: Vec<String> = e.raw[..0xd0]
                    .chunks(4)
                    .map(|c| {
                        let b: [u8; 4] = c.try_into().unwrap();
                        let i = i32::from_le_bytes(b);
                        let f = f32::from_le_bytes(b);
                        if i == 0 { "0".into() } else if i.unsigned_abs() < 0x100000 { format!("{i}") } else { format!("{f:.3}") }
                    })
                    .collect();
                println!("{}#{i} t{} {}", fx.name.rsplit('/').next().unwrap_or(""), e.raw[0xc8], words.join(" "));
            }
        }
        return;
    }
    for fx in w.capture.fx.iter().filter(|f| f.name.contains(&want)) {
        let h = &fx.header;
        let u16_at = |o: usize| u16::from_le_bytes([h[o], h[o + 1]]) as i16;
        let i32_at = |o: usize| i32::from_le_bytes(h[o..o + 4].try_into().unwrap());
        println!(
            "{}  flags {:#x} looping {} oneshot {} emission {} loop life {} life {}",
            fx.name,
            u16_at(8),
            u16_at(0xc),
            u16_at(0xe),
            u16_at(0x10),
            i32_at(0x18),
            i32_at(0x1c)
        );
        for (i, e) in fx.elems.iter().enumerate() {
            let r = &e.raw;
            println!(
                "  [{i}] type {} visuals {} counts {} {} {} {}  {:?} / {:?}  fx {:?}{}",
                r[0xc8],
                r[0xc9],
                r[0xca],
                r[0xcb],
                r[0xcc],
                r[0xcd],
                e.visuals,
                e.visuals2,
                e.effects.iter().flatten().collect::<Vec<_>>(),
                if e.trail.is_some() { " trail" } else { "" }
            );
            if raw {
                for row in 0..(0xd0 / 16) {
                    let o = row * 16;
                    let words: Vec<String> = (0..4)
                        .map(|k| {
                            let b: [u8; 4] = r[o + 4 * k..o + 4 * k + 4].try_into().unwrap();
                            let i = i32::from_le_bytes(b);
                            let f = f32::from_le_bytes(b);
                            if i.unsigned_abs() < 0x100000 { format!("{i:>10}") } else { format!("{f:>10.4}") }
                        })
                        .collect();
                    println!("      {o:03x}: {}", words.join(" "));
                }
                for o in [0xf0usize, 0x130, 0x1b0, 0x1d8, 0x1f0, 0x220, 0x240, 0x250] {
                    let words: Vec<String> = (0..4)
                        .map(|k| {
                            let b: [u8; 4] = r[o + 4 * k..o + 4 * k + 4].try_into().unwrap();
                            format!("{:08x}", u32::from_le_bytes(b))
                        })
                        .collect();
                    println!("      {o:03x}: {}", words.join(" "));
                }
            }
            // T7_FX_HEX=1: bytes 0xa8..0xd0 and 0x1b0..0x260 as hex.
            if std::env::var_os("T7_FX_HEX").is_some() {
                let hx = |a: usize, b: usize| r[a..b].iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("");
                println!("      a8: {}", hx(0xa8, 0xd0));
                for o in (0x1b0..0x260).step_by(0x30) {
                    println!("      {o:03x}: {}", hx(o, (o + 0x30).min(0x260)));
                }
            }
            // T7_FX_VEL=1: flags words, then the velocity samples' first and last rows (24 words: local vel base/amp, delta base/amp, world ...).
            if std::env::var_os("T7_FX_VEL").is_some() {
                let w = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                println!("      flags {:08x} {:08x}", w(0), w(4));
                let rows: Vec<&[u8]> = e.samples[0].chunks(96).collect();
                for (k, rec) in rows.iter().enumerate().filter(|(k, _)| *k == 0 || *k + 1 == rows.len()) {
                    let f: Vec<String> = rec.chunks(4).map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap()))).collect();
                    println!("      vel{k:02}: L {} | {} || W {} | {}", f[0..6].join(" "), f[6..12].join(" "), f[12..18].join(" "), f[18..24].join(" "));
                }
            }
            // T7_FX_VIS=1: the visual samples (s2) as 10-word rows (base, then amplitude), hex.
            if std::env::var_os("T7_FX_VIS").is_some() {
                for (k, rec) in e.samples[2].chunks(80).enumerate() {
                    let w: Vec<String> = rec.chunks(4).map(|c| {
                        let b: [u8; 4] = c.try_into().unwrap();
                        let f = f32::from_le_bytes(b);
                        let u = u32::from_le_bytes(b);
                        if u == 0 { "0".into() } else if f.abs() > 1e-4 && f.abs() < 1e6 { format!("{f:.3}") } else { format!("#{u:08x}") }
                    }).collect();
                    println!("      v{k:02}: {} | {}", w[..10].join(" "), w[10..].join(" "));
                }
            }
            if samples {
                for (k, s) in e.samples.iter().enumerate() {
                    let fl: Vec<String> = s.chunks(4).map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap()))).collect();
                    println!("      s{k}: {}", fl.join(" "));
                }
            }
        }
    }
}
