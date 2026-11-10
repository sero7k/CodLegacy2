//! Every effect of a zone played by Modern Warfare Remastered's particle
//! rules (`fx_iw4::h1`, read off the game's executable) and checked:
//! `cargo run -p fastfile_h1 --release --example h1_fx_check -- <zone.ff>...`
//!
//! Each effect is started at 0 ms with three effect seeds; its looping
//! elements spawn over 10 s on their interval (up to their count), its
//! one-shot elements their drawn count; every particle is evaluated at five
//! points of its life. Checked: seeds in range, begin and life inside the
//! definition's ranges, every visual channel finite and inside the
//! envelope its samples allow, the fade in 0..1. Reported: particles alive
//! at 5 s, what the game leaves undrawn, and how far the converted path's
//! colour (bytes, emissive folded in and squeezed) is from the game's.

use fx_iw4::h1::{self, H1ElemRules, vis};

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// The converted path's colour for one sample half pair (as
/// `asset_game::s1_vis_sample_to_iw4`): colour x emissive when emissive > 0,
/// then base and second half squeezed by the brighter's peak.
fn old_bytes(s: &[u8]) -> [[f32; 4]; 2] {
    let lit = |half: usize| -> [f32; 4] {
        let mut c: [f32; 4] = core::array::from_fn(|k| f32_at(s, half + vis::COLOR + 4 * k));
        for (k, ch) in c.iter_mut().take(3).enumerate() {
            let e = f32_at(s, half + vis::EMISSIVE + 4 * k);
            if e.is_finite() && e > 0.0 {
                *ch *= e;
            }
        }
        c
    };
    let (b, a) = (lit(0), lit(vis::HALF));
    let peak = (0..3).map(|k| b[k] + a[k].max(0.0)).fold(0.0f32, f32::max);
    let q = if peak > 1.0 && peak.is_finite() { 1.0 / peak } else { 1.0 };
    let byte = |x: f32| ((x.clamp(0.0, 1.0) * 255.0).round()) / 255.0;
    let fix = |c: [f32; 4]| -> [f32; 4] { [byte(c[0] * q), byte(c[1] * q), byte(c[2] * q), byte(c[3])] };
    [fix(b), fix(a)]
}

#[derive(Default)]
struct Tally {
    effects: usize,
    elems: usize,
    particles: usize,
    evaluations: usize,
    alive_5s: usize,
    failures: Vec<String>,
    faded_undrawn: usize,
    zero_alpha: usize,
    hdr_over_1: usize,
    colour_diff_over_16: usize,
    colour_diff_sum: f64,
    lit_types: [usize; 6],
}

fn check_zone(path: &str) {
    let zone = fastfile_h1::Zone::open(std::path::Path::new(path)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
    let mut t = Tally::default();
    for f in cap.fx.iter().filter(|f| !f.name.starts_with(',')) {
        t.effects += 1;
        let (n_loop, n_one) = (f.elem_counts.0.max(0) as usize, f.elem_counts.1.max(0) as usize);
        for (k, el) in f.elems.iter().enumerate() {
            let r = el.record.get(&zone.image);
            let rules = H1ElemRules::read(r);
            // Sprite-like elements (billboard, oriented, rotated, tail, line,
            // flare, cloud): the ones with visual samples drawn as quads.
            if !matches!(rules.elem_type, 0..=4 | 6 | 8) || k >= n_loop + n_one {
                continue;
            }
            let Some(samples) = el.vis_samples.map(|s| s.get(&zone.image)) else { continue };
            t.elems += 1;
            t.lit_types[usize::from(rules.lit_type.min(5))] += 1;
            // The envelope every channel must stay in.
            let n = usize::from(rules.vis_intervals) + 1;
            let env = |off: usize, lerp: bool| -> (f32, f32) {
                let mut lo = f32::MAX;
                let mut hi = f32::MIN;
                for i in 0..n {
                    let s = &samples[i * vis::SIZE..(i + 1) * vis::SIZE];
                    let (b, a) = (f32_at(s, off), f32_at(s, vis::HALF + off));
                    let (x, y) = if lerp { (b, a) } else { (b, b + a) };
                    lo = lo.min(x.min(y));
                    hi = hi.max(x.max(y));
                }
                (lo - 1e-3 * lo.abs().max(1.0), hi + 1e-3 * hi.abs().max(1.0))
            };
            let env_size = env(vis::SIZE0, false);
            let env_red = env(vis::COLOR, true);
            let env_alpha = env(vis::COLOR + 12, true);
            for effect_seed in [0u16, 1234, 9999] {
                let mut spawns: Vec<(i32, u32)> = Vec::new();
                if k < n_loop {
                    h1::looping_spawns(rules.spawn_a, rules.spawn_b, 0, 10_000, |m, s| spawns.push((m, s)));
                } else {
                    for s in 0..rules.one_shot_count(effect_seed).max(0) {
                        spawns.push((0, s as u32));
                    }
                }
                for (msec, sequence) in spawns {
                    let seq = sequence as u8;
                    let (begin, seed) = rules.begin(seq, effect_seed, msec);
                    let life = rules.life_msec(seed);
                    t.particles += 1;
                    let mut fail = |what: String| {
                        if t.failures.len() < 20 {
                            t.failures.push(format!("{} e{k}: {what}", f.name));
                        }
                    };
                    if seed >= h1::H1_RAND_MOD {
                        fail(format!("seed {seed}"));
                    }
                    let d0 = msec + rules.delay[0];
                    if begin < d0 || begin > d0 + rules.delay[1].max(0) {
                        fail(format!("begin {begin} outside the delay"));
                    }
                    if life < rules.life[0] || life > rules.life[0] + rules.life[1].max(0) {
                        fail(format!("life {life} outside {:?}", rules.life));
                    }
                    if life <= 0 {
                        continue;
                    }
                    if begin <= 5000 && 5000 < begin + life {
                        t.alive_5s += 1;
                    }
                    for q in [0.0f32, 0.25, 0.5, 0.75, 0.999] {
                        let age = (q * life as f32) as i32;
                        let (_, norm) = h1::draw_age(age, life);
                        let Some(v) = h1::visual_state(&rules, samples, norm, seed, life as f32) else {
                            fail("samples short".to_owned());
                            break;
                        };
                        t.evaluations += 1;
                        let all = v.rgba.iter().chain(&v.emissive).chain(&v.size).chain(&v.pivot);
                        if !all.clone().all(|x| x.is_finite()) || !v.rotation.is_finite() || !v.scale.is_finite() {
                            fail(format!("not finite at {q}: {v:?}"));
                            continue;
                        }
                        if v.size[0] < env_size.0 || v.size[0] > env_size.1 {
                            fail(format!("size {} outside {env_size:?}", v.size[0]));
                        }
                        if v.rgba[0] < env_red.0 || v.rgba[0] > env_red.1 || v.rgba[3] < env_alpha.0 || v.rgba[3] > env_alpha.1 {
                            fail(format!("colour {:?} outside {env_red:?} / {env_alpha:?}", v.rgba));
                        }
                        // Fades over a sweep of distances stay in 0..1.
                        for dist in [0.0f32, 64.0, 512.0, 4096.0] {
                            let fd = h1::distance_fade(dist, rules.fade_in, rules.fade_out);
                            if !(0.0..=1.0).contains(&fd) {
                                fail(format!("fade {fd} at {dist}"));
                            }
                        }
                        if h1::distance_fade(512.0, rules.fade_in, rules.fade_out) <= 0.0 {
                            t.faded_undrawn += 1;
                        } else if v.rgba[3] <= 0.0 {
                            t.zero_alpha += 1;
                        }
                        let hdr = h1::lit_rgb(&rules, [v.rgba[0], v.rgba[1], v.rgba[2]], v.emissive, [1.0; 3], true);
                        if hdr.iter().any(|&x| x > 1.0) {
                            t.hdr_over_1 += 1;
                        }
                        // The converted path's colour at the same point.
                        let s = h1::vis_setup(rules.vis_intervals, norm);
                        let o0 = old_bytes(&samples[s.index * vis::SIZE..(s.index + 1) * vis::SIZE]);
                        let o1 = old_bytes(&samples[(s.index + 1) * vis::SIZE..(s.index + 2) * vis::SIZE]);
                        // (The converted path's colour random comes from IW4's
                        // table; the game's random is used for both so only
                        // the colour rule differs.)
                        let c = h1::rand_f32(seed, h1::ch::COLOR);
                        let old: [f32; 3] = core::array::from_fn(|i| {
                            (o0[0][i] * (1.0 - c) + o0[1][i] * c) * (1.0 - s.frac) + (o1[0][i] * (1.0 - c) + o1[1][i] * c) * s.frac
                        });
                        let new = h1::display_rgb(hdr);
                        let diff = (0..3).map(|i| (old[i] - new[i]).abs()).fold(0.0f32, f32::max);
                        t.colour_diff_sum += f64::from(diff);
                        if diff * 255.0 > 16.0 {
                            t.colour_diff_over_16 += 1;
                        }
                    }
                }
            }
        }
    }
    println!("== {path}");
    println!(
        "{} effects, {} sprite elements (lit types 0..5+: {:?}), {} particles, {} evaluations, {} alive at 5 s",
        t.effects, t.elems, t.lit_types, t.particles, t.evaluations, t.alive_5s
    );
    println!(
        "failures: {}{}",
        t.failures.len(),
        if t.failures.is_empty() { String::new() } else { format!(": {:#?}", t.failures) }
    );
    let e = t.evaluations.max(1) as f64;
    println!(
        "faded out at 512 units: {:.1}%, zero alpha: {:.1}%, lit HDR colour above 1: {:.1}% (unlit light 1.0)",
        100.0 * t.faded_undrawn as f64 / e,
        100.0 * t.zero_alpha as f64 / e,
        100.0 * t.hdr_over_1 as f64 / e
    );
    println!(
        "converted path's colour against the game's (display, light 1.0): mean {:.1}/255, {:.1}% more than 16/255 apart",
        255.0 * t.colour_diff_sum / e,
        100.0 * t.colour_diff_over_16 as f64 / e
    );
    let _ = i32_at;
}

fn main() {
    for path in std::env::args().skip(1) {
        check_zone(&path);
    }
}
