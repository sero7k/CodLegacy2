//! A Ghosts zone effect's motion terms as the runtime reads them (flags,
//! gravity, velocity samples, spawn angles): `iw6_fx_motion <zone.ff> <substring>`.
fn f(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4)
        .map_or(0.0, |s| f32::from_le_bytes(s.try_into().unwrap()))
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&args[0])).expect("zone");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    if let Some(fx) = fastfile_iw6::createfx::read(&cap, &zone.image) {
        for p in fx
            .placed
            .iter()
            .filter(|p| p.fxid.contains(args[1].as_str()))
            .take(6)
        {
            println!("placed {} at {:?} angles {:?}", p.fxid, p.origin, p.angles);
        }
    }
    for r in &cap.fx {
        if r.name.starts_with(',') || !r.name.contains(args[1].as_str()) {
            continue;
        }
        let o = fastfile_iw6::fxview::own(r, &cap, &zone.image);
        println!("{} ({} elems, loops {})", o.name, o.elems.len(), o.loops());
        for (i, e) in o.elems.iter().enumerate() {
            let rec = &e.record;
            let flags = i32::from_le_bytes(rec[0..4].try_into().unwrap());
            println!(
                "  [{i}] type {} flags {flags:#010x} gravity {:?} angles {:?} life {:?} velN {}",
                rec[180],
                [f(rec, 156), f(rec, 160)],
                (0..3)
                    .map(|a| [f(rec, 100 + a * 8), f(rec, 104 + a * 8)])
                    .collect::<Vec<_>>(),
                [f(rec, 52), f(rec, 56)],
                rec[183] + 1
            );
            for s in 0..=(rec[183] as usize) {
                let b = s * 96;
                println!(
                    "      local v {:?} +{:?}  world v {:?} +{:?}",
                    [
                        f(&e.vel_samples, b),
                        f(&e.vel_samples, b + 4),
                        f(&e.vel_samples, b + 8)
                    ],
                    [
                        f(&e.vel_samples, b + 12),
                        f(&e.vel_samples, b + 16),
                        f(&e.vel_samples, b + 20)
                    ],
                    [
                        f(&e.vel_samples, b + 48),
                        f(&e.vel_samples, b + 52),
                        f(&e.vel_samples, b + 56)
                    ],
                    [
                        f(&e.vel_samples, b + 60),
                        f(&e.vel_samples, b + 64),
                        f(&e.vel_samples, b + 68)
                    ],
                );
            }
        }
    }
}
