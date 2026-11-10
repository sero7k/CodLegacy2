//! A particle system's body: `iw7_vfxdump <zone.ff> <name part>`: emitters
//! (spawn rate, life, burst), states, and each module with its type, linked
//! assets and curves. `IW7_VFX_HEX=1` also prints module data words.
//! With no name part: counts of module types over every system.
use fastfile_iw7::walk::walk_fitted;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&args[1])).expect("zone");
    let (w, stop) = walk_fitted(&zone);
    if let Some(stop) = stop {
        eprintln!("walk stopped: {stop:?}");
    }
    let cap = &w.cap;
    let Some(part) = args.get(2) else {
        let mut counts = std::collections::BTreeMap::new();
        for v in &cap.vfx {
            for e in &v.emitters {
                for s in &e.states {
                    for g in &s.groups {
                        for m in g {
                            *counts.entry(m.ty).or_insert(0usize) += 1;
                        }
                    }
                }
            }
        }
        println!("{} systems", cap.vfx.len());
        for (t, n) in counts {
            println!("  module {t:#04x}: {n}");
        }
        return;
    };
    let hex = std::env::var_os("IW7_VFX_HEX").is_some();
    for v in cap.vfx.iter().filter(|v| v.name.contains(part.as_str())) {
        println!(
            "{} (version {}, flags {:#x}, {} emitters)",
            v.name,
            v.version,
            v.flags,
            v.emitters.len()
        );
        for (i, e) in v.emitters.iter().enumerate() {
            use fastfile_iw7::layout::ParticleEmitterDef as E;
            println!(
                "  emitter {i}: rate {:?} life {:?} delay {:?} max {} burst {:?} emitterLife {:?} emitterDelay {:?} flags {:#x} range {:?}, fade scale {} {:?}",
                [
                    e.f32_at(E::particleSpawnRate__min),
                    e.f32_at(E::particleSpawnRate__max)
                ],
                [
                    e.f32_at(E::particleLife__min),
                    e.f32_at(E::particleLife__max)
                ],
                [
                    e.f32_at(E::particleDelay__min),
                    e.f32_at(E::particleDelay__max)
                ],
                e.i32_at(E::particleCountMax),
                [
                    e.i32_at(E::particleBurstCount__min),
                    e.i32_at(E::particleBurstCount__max)
                ],
                [e.f32_at(E::emitterLife__min), e.f32_at(E::emitterLife__max)],
                [
                    e.f32_at(E::emitterDelay__min),
                    e.f32_at(E::emitterDelay__max)
                ],
                e.i32_at(E::flags),
                [
                    e.f32_at(E::spawnRangeSq__min),
                    e.f32_at(E::spawnRangeSq__max)
                ],
                e.fade.scale,
                e.fade.points
            );
            for (si, s) in e.states.iter().enumerate() {
                println!(
                    "    state {si}: elementType {} flags {:#x}",
                    s.element_type, s.flags
                );
                for (gi, g) in s.groups.iter().enumerate() {
                    for m in g {
                        let links: Vec<String> = m
                            .links
                            .iter()
                            .map(|l| match l {
                                fastfile_iw7::walk::vfx::LinkRec::Asset(id) => {
                                    format!("{:?}:{}", id.ty, cap.name_of(*id))
                                }
                                fastfile_iw7::walk::vfx::LinkRec::Sound(s) => format!("snd:{s}"),
                            })
                            .collect();
                        println!(
                            "      g{gi} module {:#04x} flags {:#x} {links:?}",
                            m.ty,
                            m.u32_at(4)
                        );
                        if hex {
                            let words: Vec<String> = m
                                .data
                                .chunks_exact(4)
                                .take(28)
                                .map(|c| {
                                    let u = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                                    let f = f32::from_bits(u);
                                    if f.is_finite() && f.abs() > 1e-6 && f.abs() < 1e7 {
                                        format!("{f}")
                                    } else {
                                        format!("{u:#x}")
                                    }
                                })
                                .collect();
                            println!("        data {}", words.join(" "));
                        }
                        for (ci, c) in m.curves.iter().enumerate() {
                            if !c.points.is_empty() {
                                println!("        curve {ci} scale {} {:?}", c.scale, c.points);
                            }
                        }
                    }
                }
            }
        }
    }
}
