//! Which of a light probe's 27 halves is which spherical-harmonic term:
//! `iw7_probe_sh <map.ff>`. Each layout (channel-major or term-major) and each
//! signed assignment of the three first-order terms to x, y, z is scored by
//! how well the first-order vector of the probes in the sun points at the sun
//! (the ComWorld's sun direction, either sign) and by the sky being above.
use fastfile_iw7::map::{Iw7Map, MapSources};

fn half(h: u16) -> f32 {
    let e = i32::from((h >> 10) & 0x1f);
    let m = f32::from(h & 0x3ff);
    let v = if e == 0 { m * 2f32.powi(-24) } else if e == 31 { 0.0 } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
    if h & 0x8000 != 0 { -v } else { v }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: iw7_probe_sh <map.ff>");
    let src = MapSources::beside(std::path::Path::new(&path)).with_common();
    let map = Iw7Map::load(&src).expect("load");
    let sun = map.lights.iter().find(|l| l.kind == 1).expect("sun");
    println!("{} probes; sun dir {:?} colour {:?}", map.light_probes.len(), sun.dir, sun.color);
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        v.map(|x| x / l)
    };
    let sdir = norm(sun.dir);
    // Per probe: 27 halves, and the visibility.
    let probes: Vec<([f32; 27], f32)> = map
        .light_probes
        .iter()
        .map(|(_, d)| {
            let h: Vec<f32> = (0..28).map(|i| half(u16::from_le_bytes([d[2 * i], d[2 * i + 1]]))).collect();
            let mut a = [0f32; 27];
            a.copy_from_slice(&h[..27]);
            (a, h[27])
        })
        .filter(|(a, _)| a.iter().all(|v| v.is_finite()))
        .collect();
    let lit: Vec<_> = probes.iter().filter(|p| p.1 > 0.9).collect();
    let shade: Vec<_> = probes.iter().filter(|p| p.1 < 0.1).collect();
    println!("{} usable, {} in the sun (vis > 0.9), {} in shade", probes.len(), lit.len(), shade.len());
    // coefficient (term t 0..9, channel c 0..3) -> index, by layout
    let idx = |layout: usize, t: usize, c: usize| if layout == 0 { c * 9 + t } else { t * 3 + c };
    let lum = |p: &[f32; 27], layout: usize, t: usize| 0.3 * p[idx(layout, t, 0)] + 0.59 * p[idx(layout, t, 1)] + 0.11 * p[idx(layout, t, 2)];
    // The mean first-order triple (terms 1..3, term-major layout) of the
    // probes in the sun, against the sun's direction.
    for (name, set) in [("sun", &lit), ("shade", &shade)] {
        let mut m = [0f32; 3];
        for p in set.iter() {
            let v = norm([lum(&p.0, 0, 1), lum(&p.0, 0, 2), lum(&p.0, 0, 3)]);
            for k in 0..3 { m[k] += v[k] / set.len().max(1) as f32; }
        }
        println!("mean unit first-order triple (terms 1,2,3) of probes in {name}: {m:+.3?}");
    }
    let mut rows = Vec::new();
    // (Layout 1, term-major, puts the green DC term in the first-order slots and
    // wins by accident; the halves are channel-major, as the report's dump shows.)
    for layout in 0..1 {
        for perm in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            for signs in 0..8u32 {
                let s = [0, 1, 2].map(|k| if signs >> k & 1 == 1 { -1.0 } else { 1.0 });
                // axis a (x, y, z) takes term 1 + perm[a] with sign s[a]
                let axis = |p: &[f32; 27]| norm([0, 1, 2].map(|a| s[a] * lum(p, layout, 1 + perm[a])));
                let mean_dot = |set: &[&([f32; 27], f32)], d: [f32; 3]| {
                    set.iter().map(|p| { let v = axis(&p.0); v[0] * d[0] + v[1] * d[1] + v[2] * d[2] }).sum::<f32>() / set.len().max(1) as f32
                };
                let up = |set: &[&([f32; 27], f32)]| set.iter().map(|p| axis(&p.0)[2]).sum::<f32>() / set.len().max(1) as f32;
                let all: Vec<_> = probes.iter().collect();
                rows.push((layout, perm, s, mean_dot(&lit, sdir), mean_dot(&lit, sdir.map(|v| -v)), up(&all), up(&shade)));
            }
        }
    }
    rows.sort_by(|a, b| b.3.max(b.4).total_cmp(&a.3.max(a.4)));
    println!("layout (0 channel-major, 1 term-major); axis x,y,z take first-order term perm+1 with signs; dot with sun dir, with -dir; mean up (all), mean up (shade)");
    // The vertex shader's own basis (disassembled from `lprobe` vertex programs):
    // the direction's (x, z, y) feed the terms (1, 2, 3) with -0.4886, +0.4886,
    // -0.4886, so a first-order vector (c1, c2, c3) points at (-c1, -c3, +c2).
    if let Some(r) = rows.iter().find(|r| r.1 == [0, 2, 1] && r.2 == [-1.0, -1.0, 1.0]) {
        println!("shader's mapping x=-t1 y=-t3 z=+t2: +dir {:+.3} up {:+.3} up(shade) {:+.3}", r.3, r.5, r.6);
    }
    for r in rows.iter().take(8) {
        println!("  layout {} perm {:?} signs {:?}: +dir {:+.3} -dir {:+.3} up {:+.3} up(shade) {:+.3}", r.0, r.1, r.2, r.3, r.4, r.5, r.6);
    }
}
