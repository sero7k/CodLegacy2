//! The zombies' locomotion clips' root motion (`walk::anim`'s delta
//! translation) read from Shadows of Evil's zone: the ground a walk, run,
//! sprint and super sprint clip covers a second is Black Ops III's zombie
//! speed. Opt-in (`T7_ZONES=<zone dir>`; the zone is 228 MB, walk it in a
//! release build).

use fastfile_t7::walk::{Options, walk_with};

#[test]
fn zombie_locomotion_clips_cover_black_ops_iii_speeds() {
    let Some(dir) = std::env::var_os("T7_ZONES") else {
        eprintln!("T7_ZONES not set; skipped");
        return;
    };
    let path = std::path::Path::new(&dir).join("zm_zod.ff");
    let Ok(zone) = fastfile_t7::Zone::open(&path) else {
        eprintln!("no zm_zod.ff; skipped");
        return;
    };
    let w = walk_with(&zone, Options { keep_anims: true, ..Default::default() });
    let cap = &w.capture;
    let speed = |prefix: &str| -> Vec<f32> {
        cap.anims
            .iter()
            .filter_map(|r| {
                let name = r.name.trim_start_matches(',');
                let rest = name.strip_prefix("ai_zombie_base_")?;
                let numbered = rest.strip_prefix(prefix).is_some_and(|t| t.starts_with('v') && t[1..].bytes().all(|b| b.is_ascii_digit()));
                if !numbered || r.delta_trans.len() < 2 {
                    return None;
                }
                let framerate = r.header.get(0x50..0x54).map_or(30.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                let (a, b) = (r.delta_trans.first()?.1, r.delta_trans.last()?.1);
                Some((b[0] - a[0]).hypot(b[1] - a[1]) / (f32::from(r.numframes) / framerate))
            })
            .collect()
    };
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    let (walk, run, sprint) = (speed("walk_ad_"), speed("run_ad_"), speed("sprint_ad_"));
    assert!(!walk.is_empty() && !run.is_empty() && !sprint.is_empty(), "no locomotion clips with root motion");
    // (Measured: walks 23-53, runs 57-132, sprints 140-160 units a second.)
    assert!((20.0..60.0).contains(&mean(&walk)), "walk {}", mean(&walk));
    assert!((55.0..135.0).contains(&mean(&run)), "run {}", mean(&run));
    assert!((130.0..170.0).contains(&mean(&sprint)), "sprint {}", mean(&sprint));
    assert!(mean(&walk) < mean(&run) && mean(&run) < mean(&sprint));
}
