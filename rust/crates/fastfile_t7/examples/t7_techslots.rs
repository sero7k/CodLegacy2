//! Which program each technique slot of a technique set uses (the shader
//! name the zone keeps just before each program): `t7_techslots <zone.ff>
//! [techset substring] [max sets]`.
fn shader_name(img: &[u8], at: usize) -> String {
    // The nearest printable name ending within 512 bytes before the program.
    let lo = at.saturating_sub(512);
    let win = &img[lo..at];
    let mut best = String::new();
    let mut cur = Vec::new();
    for &b in win {
        if (0x20..0x7f).contains(&b) {
            cur.push(b);
        } else {
            if cur.len() >= 6 {
                best = String::from_utf8_lossy(&cur).into_owned();
            }
            cur.clear();
        }
    }
    best
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let max: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(5);
    let w = fastfile_t7::walk::walk(&zone);
    for ts in w.capture.techsets.iter().filter(|t| t.name.contains(&want)).take(max) {
        println!("{}", ts.name);
        for (i, p) in ts.programs.iter().enumerate() {
            let Some([vs, ps]) = p else { continue };
            let n = |x: &Option<(usize, usize)>| x.map(|(at, _)| shader_name(&zone.image, at)).unwrap_or_default();
            let outs = ps.and_then(|(at, len)| {
                let prog = zone.image.get(at..at + len)?;
                let o = fastfile_t7::dxbc::chunk(prog, b"OSGN").or_else(|| fastfile_t7::dxbc::chunk(prog, b"OSG5"))?;
                Some(u32::from_le_bytes(o.get(0..4)?.try_into().ok()?))
            });
            let tex: Vec<String> = ps
                .and_then(|(at, len)| zone.image.get(at..at + len))
                .and_then(fastfile_t7::dxbc::reflect)
                .map(|r| r.textures.into_iter().map(|t| t.0).collect())
                .unwrap_or_default();
            println!("  {i:2} vs {} | ps outputs {outs:?} {tex:?}", n(vs));
        }
    }
}
