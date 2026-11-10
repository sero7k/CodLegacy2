//! bo2mp: a map's siege animations (the GPU-skinned fxanim props: vines,
//! flags, cables): `t7_siege <map.ff> [xpak dir]` prints each `SAnim`'s
//! header words, its stream buffer (key, size, and with the packages the
//! first bytes of its data), the auto-play list and which static model
//! draws each plays on.
use std::path::Path;

fn u16_at(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = Path::new(&a[1]);
    let zone = fastfile_t7::Zone::open(path).expect("zone");
    let w = fastfile_t7::walk::walk(&zone);
    let img = &zone.image;
    let paks = a.get(2).map(|d| fastfile_t7::xpak::XpakSet::open_dir(Path::new(d)).map(|(p, _)| p));
    for s in &w.capture.world.sanims {
        let h = s.header_at;
        println!(
            "sanim {} @{:#x}: parts(0x54) {} frames(0x58) {} rate(0x5c) {} 0x60 {:#x} flags(0x64) {:#x} shots(0x66) {} events 0x68..0x6c {} {} {}; words 0x28..0x54 {:?}",
            s.name,
            h,
            u32_at(img, h + 0x54),
            u16_at(img, h + 0x58),
            f32_at(img, h + 0x5c),
            u32_at(img, h + 0x60),
            u16_at(img, h + 0x64),
            u16_at(img, h + 0x66),
            u16_at(img, h + 0x68),
            u16_at(img, h + 0x6a),
            u16_at(img, h + 0x6c),
            (0x28..0x54).step_by(4).map(|o| f32_at(img, h + o)).collect::<Vec<_>>()
        );
        if let Some(sh) = s.shots {
            let n = u16_at(img, h + 0x66).max(1) as usize;
            for i in 0..n.min(8) {
                let r = sh + 12 * i;
                println!("  shot {i}: u16s {} {} {} {} id {:#x}", u16_at(img, r), u16_at(img, r + 2), u16_at(img, r + 4), u16_at(img, r + 6), u32_at(img, r + 8));
            }
        }
        if let Some(b) = s.stream {
            let key = u64_at(img, b + 8);
            println!("  stream buffer @{b:#x}: key {key:#018x}, words {:?}", (0..0x40).step_by(4).map(|o| u32_at(img, b + o)).collect::<Vec<_>>());
            if let Some(Ok(p)) = &paks {
                match p.read(key) {
                    Ok(d) => {
                        println!("  streamed: {} bytes; first u32s {:?}", d.len(), (0..0x80.min(d.len())).step_by(4).map(|o| u32_at(&d, o)).collect::<Vec<_>>());
                        println!("  as f32 {:?}", (0x40..0x100.min(d.len())).step_by(4).map(|o| f32_at(&d, o)).collect::<Vec<_>>());
                    }
                    Err(e) => println!("  streamed: {e}"),
                }
            }
        }
        if let Some(d) = s.data {
            println!("  resident: {:?}", d);
            // How unit the rotation keys' three quaternions are, the array
            // read at small shifts (where the walk put it, and around).
            if let Some((at, n)) = d[3] {
                for shift in [-16i64, -12, -8, -4, 0, 4, 8, 12, 16] {
                    let base = (at as i64 + shift) as usize;
                    let dev = |q: usize| -> f32 {
                        (0..n.min(400)).map(|i| {
                            let l: f32 = (0..4).map(|k| f32_at(img, base + 48 * i + 16 * q + 4 * k).powi(2)).sum::<f32>().sqrt();
                            if l.is_finite() { (l - 1.0).abs().min(10.0) } else { 10.0 }
                        }).sum::<f32>() / n.min(400) as f32
                    };
                    println!("  rot keys at {shift:+}: mean |len-1| of q0 {:.4} q1 {:.4} q2 {:.4}", dev(0), dev(1), dev(2));
                }
            }
        }
    }
    // Poses sampled from the compressed animations (`fastfile_t7::siege`):
    // per animation the largest part movement from frame 0 and the largest
    // step between neighbouring frames (a smooth animation's is small).
    for a in fastfile_t7::siege::read_all(img, &w.capture.world.sanims) {
        let Some(c) = &a.compressed else { continue };
        // `T7_SIEGE_TRACE=<name substring>:<part>`: that part's offset per frame.
        if let Some((n, k)) = std::env::var("T7_SIEGE_TRACE").ok().and_then(|v| v.split_once(':').map(|(a, b)| (a.to_owned(), b.parse::<usize>().unwrap_or(0)))) {
            if a.header.name.contains(&n) && k < c.parts.len() {
                for f in 0..usize::from(a.header.frames).min(40) {
                    let p = c.part_pose(k, f as f32);
                    println!("  trace {f}: pos {:?} quat {:?}", p.pos, p.quat);
                }
            }
        }
        let frames = usize::from(a.header.frames).max(1);
        let rest = c.pose(0.0);
        let (mut far, mut step) = (0.0f32, 0.0f32);
        let mut prev = rest.clone();
        for f in 1..frames {
            let p = c.pose(f as f32);
            for (k, pp) in p.iter().enumerate() {
                let probe = [pp.pivot[0] + 4.0, pp.pivot[1] + 4.0, pp.pivot[2]];
                let (a0, a1, b) = (rest[k].apply(probe), prev[k].apply(probe), pp.apply(probe));
                far = far.max((0..3).map(|i| (b[i] - a0[i]).powi(2)).sum::<f32>().sqrt());
                let d = (0..3).map(|i| (b[i] - a1[i]).powi(2)).sum::<f32>().sqrt();
                if d > step && std::env::var("T7_SIEGE_STEPS").is_ok() {
                    let pk = c.parts[k];
                    let rt = &c.rot_times[pk.rot_first as usize..(pk.rot_first + u32::from(pk.rot_count)) as usize];
                    let tt = &c.trans_times[pk.trans_first as usize..(pk.trans_first + u32::from(pk.trans_count)) as usize];
                    println!("  step {d:.2} at frame {f} part {k}: rot times {:?}.., trans times {:?}.., pos {:?} -> {:?}", &rt[..rt.len().min(12)], &tt[..tt.len().min(12)], prev[k].pos, pp.pos);
                }
                step = step.max(d);
            }
            prev = p;
        }
        println!("pose {}: {} parts, {frames} frames, loop {}: moves up to {far:.2} units from frame 0, at most {step:.3} a frame", a.header.name, c.parts.len(), a.header.looping);
    }
    for g in &w.capture.world.gfx_worlds {
        println!("siege list: {:?}", g.siege_anims);
        if let Some((at, n)) = g.siege_autoplay {
            for i in 0..n {
                let r = at + 20 * i;
                let id = u32_at(img, r);
                let models: Vec<String> = match g.smodels {
                    Some((t, k)) => (0..k)
                        .filter(|&j| u32_at(img, t + fastfile_t7::walk::gfxworld::SMODEL_DRAW * j + 0x68) == id)
                        .map(|j| g.smodel_models.get(j).cloned().flatten().unwrap_or_default())
                        .collect(),
                    None => Vec::new(),
                };
                println!(
                    "autoplay {i}: id {id:#x} anim {} start {} shot {:#x} w4 {:#x}: {} draws {:?}",
                    u32_at(img, r + 4),
                    f32_at(img, r + 8),
                    u32_at(img, r + 12),
                    u32_at(img, r + 16),
                    models.len(),
                    &models[..models.len().min(4)]
                );
            }
        }
        let with: Vec<_> = g.smodel_sanims.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|s| (i, s.clone()))).collect();
        println!("static model draws naming a siege animation: {} {:?}", with.len(), &with[..with.len().min(6)]);
        if let Some((t, k)) = g.smodels {
            let mut ids: std::collections::BTreeMap<u32, usize> = Default::default();
            for j in 0..k {
                *ids.entry(u32_at(img, t + fastfile_t7::walk::gfxworld::SMODEL_DRAW * j + 0x68)).or_default() += 1;
            }
            println!("static model ids (+0x68): {} distinct, first {:?}", ids.len(), ids.iter().take(8).collect::<Vec<_>>());
        }
    }
}
